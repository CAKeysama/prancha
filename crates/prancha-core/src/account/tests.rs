use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};

use super::api::{HttpRequest, HttpResponse, Project};
use super::*;
use crate::extensions::net::NetError;
use crate::mal::vault::TestVault;

/// Nuvem falsa: responde por endereço e anota o que recebeu.
#[derive(Clone, Default)]
pub struct FakeCloud {
    pub requests: Arc<Mutex<Vec<HttpRequest>>>,
    pub replies: Arc<Mutex<Vec<(String, u16, Value)>>>,
}

impl FakeCloud {
    pub fn reply(&self, path: &str, status: u16, body: Value) {
        self.replies.lock().unwrap().push((path.to_string(), status, body));
    }

    pub fn calls(&self, path: &str) -> Vec<HttpRequest> {
        self.requests.lock().unwrap().iter().filter(|r| r.url.contains(path)).cloned().collect()
    }
}

impl Http for FakeCloud {
    fn send(&self, _project: &Project, request: HttpRequest) -> Result<HttpResponse, NetError> {
        self.requests.lock().unwrap().push(request.clone());
        let mut replies = self.replies.lock().unwrap();
        let Some(i) = replies.iter().position(|(path, _, _)| request.url.contains(path.as_str())) else {
            return Err(NetError::Network("sem resposta preparada".into()));
        };
        let (_, status, body) = replies.remove(i);
        Ok(HttpResponse { status, body: body.to_string().into_bytes() })
    }
}

pub fn session(access: &str, refresh: &str, user: &str) -> Value {
    json!({
        "access_token": access, "refresh_token": refresh, "expires_in": 3600, "token_type": "bearer",
        "user": { "id": user, "email": "ana@exemplo.com", "app_metadata": { "provider": "google" }, "user_metadata": { "full_name": "Ana" } }
    })
}

pub fn account(dir: &std::path::Path, cloud: &FakeCloud) -> Account {
    let project = Project { url: "https://nuvem.teste".into(), key: "chave".into() };
    Account::new(dir.join("t.db"), project, Box::new(cloud.clone()), Box::new(TestVault), Arc::new(|_, _| {})).with_port(0, Duration::from_secs(10))
}

/// Entra de verdade pelo endereço local: o "navegador" segue o endereço de autorização até o retorno.
pub fn sign_in(account: &Account, cloud: &FakeCloud, user: &str) {
    cloud.reply("grant_type=pkce", 200, session("acesso-1", "renova-1", user));
    let browser = |url: &str| -> AppResult<()> {
        let url = Url::parse(url).unwrap();
        let redirect = url.query_pairs().find(|(k, _)| k == "redirect_to").unwrap().1.into_owned();
        let port = Url::parse(&redirect).unwrap().port().unwrap();
        std::thread::spawn(move || {
            let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
            write!(stream, "GET {REDIRECT_PATH}?code=codigo-certo HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
            let mut out = String::new();
            let _ = stream.read_to_string(&mut out);
        });
        Ok(())
    };
    account.sign_in("google", &browser).unwrap();
}

fn stored_secret(conn: &Connection) -> Vec<u8> {
    conn.query_row("SELECT secret FROM sync_account", [], |r| r.get(0)).unwrap()
}

#[test]
fn entrar_guarda_a_conta_com_os_tokens_cifrados() {
    let dir = tempfile::tempdir().unwrap();
    let cloud = FakeCloud::default();
    let account = account(dir.path(), &cloud);
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    assert_eq!(account.status(&conn).unwrap().state, "signed_out");

    sign_in(&account, &cloud, "u1");
    let status = account.status(&conn).unwrap();
    assert_eq!((status.state, status.email.as_deref(), status.name.as_deref(), status.provider.as_deref()), ("signed_in", Some("ana@exemplo.com"), Some("Ana"), Some("google")));
    let exchange = &cloud.calls("grant_type=pkce")[0];
    assert_eq!(exchange.body["auth_code"], "codigo-certo");
    assert!(exchange.body["code_verifier"].as_str().unwrap().len() >= 43);
    let secret = stored_secret(&conn);
    assert!(!String::from_utf8_lossy(&secret).contains("acesso-1"), "nada vai em texto ao banco");
}

#[test]
fn o_retorno_entregue_pelo_sistema_tambem_vale() {
    let dir = tempfile::tempdir().unwrap();
    let cloud = FakeCloud::default();
    let account = Arc::new(account(dir.path(), &cloud));
    cloud.reply("grant_type=pkce", 200, session("a", "r", "u1"));
    assert!(!account.deliver_redirect("app.prancha://auth?code=x"), "sem login em andamento, nada acontece");
    let other = account.clone();
    let browser = move |_: &str| -> AppResult<()> {
        let other = other.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            assert!(!other.deliver_redirect("https://outro.site/auth?code=x"), "endereço de outro lugar não vale");
            assert!(other.deliver_redirect("app.prancha://auth?code=do-sistema"));
        });
        Ok(())
    };
    account.sign_in("google", &browser).unwrap();
    assert_eq!(cloud.calls("grant_type=pkce")[0].body["auth_code"], "do-sistema");
}

#[test]
fn chamadas_renovam_o_token_e_a_recusa_expira_a_sessao() {
    let dir = tempfile::tempdir().unwrap();
    let cloud = FakeCloud::default();
    let account = account(dir.path(), &cloud);
    sign_in(&account, &cloud, "u1");

    // O token foi recusado: renova uma vez e repete.
    cloud.reply("rpc/sync_devices", 401, json!({}));
    cloud.reply("grant_type=refresh_token", 200, session("acesso-2", "renova-2", "u1"));
    cloud.reply("rpc/sync_devices", 200, json!([]));
    assert_eq!(account.devices().unwrap(), json!([]));
    assert_eq!(cloud.calls("grant_type=refresh_token")[0].body["refresh_token"], "renova-1");
    let devices = cloud.calls("rpc/sync_devices");
    assert_eq!(devices[0].bearer.as_deref(), Some("acesso-1"));
    assert_eq!(devices[1].bearer.as_deref(), Some("acesso-2"));

    // A renovação também foi recusada: a sessão expirou e nada mais é tentado.
    cloud.reply("rpc/sync_devices", 401, json!({}));
    cloud.reply("grant_type=refresh_token", 400, json!({ "error_code": "refresh_token_not_found" }));
    assert_eq!(account.devices().unwrap_err().code(), "account_expired");
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    assert_eq!(account.status(&conn).unwrap().state, "expired");
    let before = cloud.requests.lock().unwrap().len();
    assert_eq!(account.devices().unwrap_err().code(), "account_expired");
    assert_eq!(cloud.requests.lock().unwrap().len(), before);
}

#[test]
fn sair_apaga_a_conta_e_o_que_veio_da_nuvem() {
    let dir = tempfile::tempdir().unwrap();
    let cloud = FakeCloud::default();
    let account = account(dir.path(), &cloud);
    sign_in(&account, &cloud, "u1");
    let conn = db::open(&dir.path().join("t.db")).unwrap();
    conn.execute_batch("INSERT INTO sync_remote_series (series_key, data) VALUES ('k', '{}'); UPDATE sync_state SET cursor = 9, full_push = 0;").unwrap();
    cloud.reply("auth/v1/logout", 204, Value::Null);
    account.sign_out(&conn).unwrap();
    assert_eq!(account.status(&conn).unwrap().state, "signed_out");
    let (remote, cursor, full): (i64, i64, i64) =
        conn.query_row("SELECT (SELECT count(*) FROM sync_remote_series), cursor, full_push FROM sync_state", [], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap();
    assert_eq!((remote, cursor, full), (0, 0, 1));
    assert_eq!(cloud.calls("auth/v1/logout").len(), 1);
}

#[test]
fn recusa_e_cancelamento_terminam_o_login() {
    let dir = tempfile::tempdir().unwrap();
    let cloud = FakeCloud::default();
    let account = account(dir.path(), &cloud);
    let refuse = |url: &str| -> AppResult<()> {
        let redirect = Url::parse(url).unwrap().query_pairs().find(|(k, _)| k == "redirect_to").unwrap().1.into_owned();
        let port = Url::parse(&redirect).unwrap().port().unwrap();
        std::thread::spawn(move || {
            let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
            write!(stream, "GET {REDIRECT_PATH}?error=access_denied HTTP/1.1\r\n\r\n").unwrap();
            let _ = stream.read_to_string(&mut String::new());
        });
        Ok(())
    };
    assert_eq!(account.sign_in("google", &refuse).unwrap_err().code(), "account_denied");
    assert_eq!(account.sign_in("outro", &refuse).unwrap_err().code(), "bad_request");
}
