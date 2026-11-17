//! Testes da integração com o MAL, com a rede simulada e um cofre de teste. Nenhuma chamada sai do computador.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream};

use super::api::{HttpRequest, HttpResponse, Method};
use super::vault::TestVault;
use super::*;
use crate::extensions::net::NetError;
use crate::extensions::reading;

type Handler = Box<dyn FnMut(&HttpRequest) -> Result<HttpResponse, NetError> + Send>;

/// Rede falsa: guarda cada requisição e responde com o que o teste mandar.
#[derive(Clone)]
struct Fake(Arc<Mutex<(Vec<HttpRequest>, Handler)>>);

impl Fake {
    fn new(handler: impl FnMut(&HttpRequest) -> Result<HttpResponse, NetError> + Send + 'static) -> Fake {
        Fake(Arc::new(Mutex::new((Vec::new(), Box::new(handler)))))
    }
    fn set(&self, handler: impl FnMut(&HttpRequest) -> Result<HttpResponse, NetError> + Send + 'static) {
        lock(&self.0).1 = Box::new(handler);
    }
    fn requests(&self) -> Vec<HttpRequest> {
        lock(&self.0).0.clone()
    }
    fn clear(&self) {
        lock(&self.0).0.clear();
    }
}

impl Http for Fake {
    fn send(&self, request: HttpRequest) -> Result<HttpResponse, NetError> {
        let mut inner = lock(&self.0);
        inner.0.push(request.clone());
        (inner.1)(&request)
    }
}

fn ok(body: serde_json::Value) -> Result<HttpResponse, NetError> {
    Ok(HttpResponse { status: 200, body: body.to_string().into_bytes() })
}

fn status(code: u16) -> Result<HttpResponse, NetError> {
    Ok(HttpResponse { status: code, body: br#"{"error":"invalid_token"}"#.to_vec() })
}

fn form_of(request: &HttpRequest) -> HashMap<String, String> {
    request.form.as_ref().map(|f| url::form_urlencoded::parse(f.0.as_bytes()).into_owned().collect()).unwrap_or_default()
}

fn bearer(request: &HttpRequest) -> Option<String> {
    match &request.auth {
        api::Auth::Bearer(t) => Some(t.clone()),
        _ => None,
    }
}

struct Env {
    _dir: tempfile::TempDir,
    mal: Mal,
    fake: Fake,
    events: Arc<Mutex<usize>>,
}

impl Env {
    fn db(&self) -> Connection {
        db::open(&self.mal.db_path).unwrap()
    }
}

/// Banco com uma obra ("Berserk", 20 capítulos numerados) e o MAL simulado.
fn env(handler: impl FnMut(&HttpRequest) -> Result<HttpResponse, NetError> + Send + 'static) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("prancha.db");
    let conn = db::open(&path).unwrap();
    conn.execute_batch(
        "INSERT INTO online_series (id, extension_id, source_id, remote_id, title, author, created_at, updated_at) VALUES (1, 'e', 's', 'berserk', 'Berserk', 'Kentaro Miura', 1, 1);
         INSERT INTO online_series (id, extension_id, source_id, remote_id, title, created_at, updated_at) VALUES (2, 'e', 's', 'monster', 'Monster', 1, 1);",
    )
    .unwrap();
    for n in 1..=20 {
        conn.execute("INSERT INTO online_chapters (id, series_id, remote_id, title, number, sort_order) VALUES (?1, 1, ?2, ?3, ?4, ?1)", params![n, format!("c{n}"), format!("Capítulo {n}"), n.to_string()]).unwrap();
    }
    let fake = Fake::new(handler);
    let events = Arc::new(Mutex::new(0usize));
    let counter = events.clone();
    let mut mal = Mal::new(path, Some("cliente".into()), Box::new(fake.clone()), Box::new(TestVault), Arc::new(move |name, _| {
        if name == EVENT_CHANGED {
            *lock(&counter) += 1;
        }
    }));
    mal.redirect_port = 0;
    mal.authorize_timeout = Duration::from_secs(10);
    mal.spacing = Duration::ZERO;
    Env { _dir: dir, mal, fake, events }
}

fn token_json(access: &str, refresh: &str, expires_in: i64) -> serde_json::Value {
    json!({ "token_type": "Bearer", "access_token": access, "refresh_token": refresh, "expires_in": expires_in })
}

/// Conecta como se a pessoa autorizasse no navegador.
fn connect(env: &Env) -> Status {
    env.mal
        .connect(&|url| {
            let url = reqwest::Url::parse(url).unwrap();
            let q: HashMap<String, String> = url.query_pairs().into_owned().collect();
            let redirect = reqwest::Url::parse(&q["redirect_uri"]).unwrap();
            let target = format!("{}?code=codigo-123&state={}", redirect.path(), q["state"]);
            let port = redirect.port().unwrap();
            std::thread::spawn(move || {
                let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
                write!(s, "GET {target} HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
                let mut out = String::new();
                let _ = s.read_to_string(&mut out);
            });
            Ok(())
        })
        .unwrap()
}

/// O MAL de mentira: conta, tokens, catálogo e a lista da pessoa, que começa com `remote`.
fn mal_server(remote: Arc<Mutex<Option<(String, i64)>>>, total: i64) -> impl FnMut(&HttpRequest) -> Result<HttpResponse, NetError> + Send + 'static {
    move |r: &HttpRequest| {
        if r.url == api::TOKEN_URL {
            return ok(token_json("acesso-1", "renova-1", 2_678_400));
        }
        if r.url.contains("/users/@me") {
            return ok(json!({ "id": 9, "name": "leitora" }));
        }
        if r.url.contains("/my_list_status") {
            let f = form_of(r);
            let mut current = lock(&remote);
            let status = f.get("status").cloned().or_else(|| current.as_ref().map(|c| c.0.clone())).unwrap_or_default();
            *current = Some((status, f["num_chapters_read"].parse().unwrap()));
            return ok(json!({}));
        }
        if r.url.contains("my_list_status") {
            let current = lock(&remote).clone();
            let mut body = json!({ "id": 2, "title": "Berserk", "num_chapters": total });
            if let Some((status, read)) = current {
                body["my_list_status"] = json!({ "status": status, "num_chapters_read": read, "is_rereading": false });
            }
            return ok(body);
        }
        if r.url.contains("/manga?") {
            return ok(json!({ "data": [
                { "node": { "id": 2, "title": "Berserk", "media_type": "manga", "num_chapters": total, "authors": [{ "node": { "first_name": "Kentaro", "last_name": "Miura" } }] } },
                { "node": { "id": 99, "title": "Berserk: The Prototype", "media_type": "one_shot", "num_chapters": 1 } }
            ]}));
        }
        if r.url.contains("/manga/") {
            return ok(json!({ "id": 2, "title": "Berserk", "media_type": "manga", "num_chapters": total }));
        }
        status(404)
    }
}

fn linked_env(remote: Option<(&str, i64)>, total: i64) -> (Env, Arc<Mutex<Option<(String, i64)>>>) {
    let remote = Arc::new(Mutex::new(remote.map(|(s, n)| (s.to_string(), n))));
    let env = env(mal_server(remote.clone(), total));
    connect(&env);
    // Com menos capítulos no MAL que na fonte, a associação automática pergunta; aí a pessoa escolhe.
    if env.mal.series_status(1, true).unwrap().state != "linked" {
        assert_eq!(env.mal.link(1, 2).unwrap().state, "linked");
    }
    env.fake.clear();
    (env, remote)
}

fn complete(env: &Env, chapter: i64) -> bool {
    let mut conn = env.db();
    reading::complete(&mut conn, chapter).unwrap();
    enqueue_completion(&conn, chapter).unwrap()
}

fn puts(env: &Env) -> Vec<HashMap<String, String>> {
    env.fake.requests().iter().filter(|r| r.method == Method::Put).map(form_of).collect()
}

fn queue(env: &Env) -> Vec<(i64, String, i64)> {
    let conn = env.db();
    let mut s = conn.prepare("SELECT chapters, status, attempts FROM mal_sync_queue ORDER BY id").unwrap();
    s.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().map(Result::unwrap).collect()
}

// ---------------------------------------------------------------------------
// Autenticação
// ---------------------------------------------------------------------------

#[test]
fn conecta_com_pkce_e_guarda_os_tokens_cifrados() {
    let remote = Arc::new(Mutex::new(None));
    let env = env(mal_server(remote, 0));
    assert_eq!(env.mal.status(&env.db()).unwrap().state, "disconnected");

    let status = connect(&env);
    assert_eq!(status.state, "connected");
    assert_eq!(status.user_name.as_deref(), Some("leitora"));

    let requests = env.fake.requests();
    let token = requests.iter().find(|r| r.url == api::TOKEN_URL).unwrap();
    let form = form_of(token);
    assert_eq!(form["grant_type"], "authorization_code");
    assert_eq!(form["code"], "codigo-123");
    assert!((43..=128).contains(&form["code_verifier"].len()));
    assert!(!form.contains_key("client_secret"), "aplicativo público: sem segredo");
    assert_eq!(bearer(requests.iter().find(|r| r.url.contains("@me")).unwrap()).as_deref(), Some("acesso-1"));

    // Nada em texto no banco.
    let secret: Vec<u8> = env.db().query_row("SELECT secret FROM mal_account", [], |r| r.get(0)).unwrap();
    let raw = String::from_utf8_lossy(&secret);
    assert!(!raw.contains("acesso-1") && !raw.contains("renova-1"));
    // E a interface nunca recebe os tokens.
    let shown = serde_json::to_string(&env.mal.status(&env.db()).unwrap()).unwrap();
    assert!(!shown.contains("acesso-1") && !shown.contains("renova-1"));
}

#[test]
fn desconectar_apaga_tokens_e_fila_mas_guarda_associacoes() {
    let (env, _) = linked_env(None, 0);
    assert!(complete(&env, 3));
    let dropped = env.mal.disconnect(&mut env.db()).unwrap();
    assert_eq!(dropped, 1);
    let status = env.mal.status(&env.db()).unwrap();
    assert_eq!(status.state, "disconnected");
    assert_eq!(status.pending, 0);
    let accounts: i64 = env.db().query_row("SELECT count(*) FROM mal_account", [], |r| r.get(0)).unwrap();
    assert_eq!(accounts, 0);
    assert_eq!(env.mal.series_status(1, false).unwrap().state, "linked");
    // Sem conta, concluir não enfileira nada e a leitura segue normal.
    assert!(!complete(&env, 4));
    assert!(*lock(&env.events) > 0, "a interface é avisada");
}

#[test]
fn sem_identificador_do_aplicativo_a_integracao_nao_aparece() {
    let dir = tempfile::tempdir().unwrap();
    let mal = Mal::new(dir.path().join("p.db"), None, Box::new(Fake::new(|_| status(500))), Box::new(TestVault), Arc::new(|_, _| {}));
    let conn = db::open(&dir.path().join("p.db")).unwrap();
    assert!(!mal.status(&conn).unwrap().available);
    assert_eq!(mal.connect(&|_| Ok(())).unwrap_err().code(), "mal_unavailable");
}

#[test]
fn renova_o_token_vencido_antes_de_usar() {
    let (env, _) = linked_env(None, 0);
    env.db().execute("UPDATE mal_account SET expires_at = 0", []).unwrap();
    let server = mal_server(Arc::new(Mutex::new(None)), 0);
    let mut server = server;
    env.fake.set(move |r| if r.url == api::TOKEN_URL { ok(token_json("acesso-2", "renova-2", 3600)) } else { server(r) });
    assert!(complete(&env, 5));
    env.mal.run_queue();
    let requests = env.fake.requests();
    let refresh = form_of(&requests[0]);
    assert_eq!(refresh["grant_type"], "refresh_token");
    assert_eq!(refresh["refresh_token"], "renova-1");
    assert!(requests[1..].iter().all(|r| bearer(r).as_deref() == Some("acesso-2")));
    let expires: i64 = env.db().query_row("SELECT expires_at FROM mal_account", [], |r| r.get(0)).unwrap();
    assert!(expires > now_ms());
    assert!(queue(&env).is_empty());
}

#[test]
fn um_401_renova_e_repete_uma_vez() {
    let (env, remote) = linked_env(None, 0);
    let mut server = mal_server(remote, 0);
    let mut first = true;
    env.fake.set(move |r| {
        if r.url == api::TOKEN_URL {
            return ok(token_json("acesso-2", "renova-2", 3600));
        }
        if bearer(r).as_deref() == Some("acesso-1") && first {
            first = false;
            return status(401);
        }
        server(r)
    });
    assert!(complete(&env, 5));
    env.mal.run_queue();
    assert!(queue(&env).is_empty());
    assert_eq!(puts(&env).len(), 1);
}

#[test]
fn renovacao_recusada_expira_a_sessao_sem_perder_a_fila() {
    let (env, _) = linked_env(None, 0);
    env.db().execute("UPDATE mal_account SET expires_at = 0", []).unwrap();
    env.fake.set(|r| if r.url == api::TOKEN_URL { status(400) } else { status(401) });
    assert!(complete(&env, 5));
    env.mal.run_queue();
    let status = env.mal.status(&env.db()).unwrap();
    assert_eq!(status.state, "expired");
    assert_eq!(queue(&env), vec![(5, "pending".into(), 0)], "a operação espera, sem gastar tentativas");
    // Expirada, a fila nem tenta.
    env.fake.clear();
    env.mal.run_queue();
    assert!(env.fake.requests().is_empty());
}

// ---------------------------------------------------------------------------
// Associação
// ---------------------------------------------------------------------------

#[test]
fn associa_sozinho_quando_ha_uma_candidata_clara() {
    let (env, _) = linked_env(None, 0);
    let status = env.mal.series_status(1, true).unwrap();
    let link = status.link.unwrap();
    assert_eq!(link.mal_id, Some(2));
    assert_eq!(link.origin, "auto");
    // Guardada: abrir de novo não pesquisa.
    assert!(env.fake.requests().is_empty());
}

#[test]
fn varias_possiveis_viram_lista_e_a_pessoa_escolhe() {
    let env = env(|r| {
        if r.url.contains("/manga?") {
            return ok(json!({ "data": [
                { "node": { "id": 10, "title": "Monster", "media_type": "manga", "num_chapters": 162 } },
                { "node": { "id": 11, "title": "Monster", "media_type": "one_shot", "num_chapters": 1 } },
                { "node": { "id": 12, "title": "Monster", "media_type": "light_novel" } }
            ]}));
        }
        if r.url.contains("/manga/11") {
            return ok(json!({ "id": 11, "title": "Monster", "media_type": "one_shot", "num_chapters": 1 }));
        }
        mal_server(Arc::new(Mutex::new(None)), 0)(r)
    });
    connect(&env);
    let status = env.mal.series_status(2, true).unwrap();
    assert_eq!(status.state, "ambiguous");
    assert!(status.link.is_none());
    let ids: Vec<i64> = status.candidates.iter().map(|c| c.mal_id).collect();
    assert_eq!(ids.len(), 2, "o romance fica de fora: {ids:?}");
    // A lista fica guardada: abrir de novo não pesquisa outra vez.
    env.fake.clear();
    assert_eq!(env.mal.series_status(2, true).unwrap().state, "ambiguous");
    assert!(env.fake.requests().is_empty());

    let chosen = env.mal.link(2, 11).unwrap();
    assert_eq!(chosen.state, "linked");
    assert_eq!(chosen.link.as_ref().unwrap().origin, "manual");
    assert_eq!(chosen.link.unwrap().mal_id, Some(11));
}

#[test]
fn corrigir_a_associacao_tira_da_fila_o_que_era_da_obra_errada() {
    let (env, _) = linked_env(None, 0);
    assert!(complete(&env, 3));
    env.fake.set(|_| ok(json!({ "id": 77, "title": "Berserk (correta)", "media_type": "manga" })));
    let status = env.mal.link(1, 77).unwrap();
    assert_eq!(status.link.unwrap().mal_id, Some(77));
    assert!(queue(&env).is_empty());
    assert!(complete(&env, 4), "novas conclusões vão para a obra certa");
    let mal_id: i64 = env.db().query_row("SELECT mal_id FROM mal_sync_queue", [], |r| r.get(0)).unwrap();
    assert_eq!(mal_id, 77);
}

#[test]
fn sem_correspondente_nao_procura_nem_sincroniza() {
    let (env, _) = linked_env(None, 0);
    let status = env.mal.unlink(1).unwrap();
    assert_eq!(status.state, "none");
    env.fake.clear();
    assert_eq!(env.mal.series_status(1, true).unwrap().state, "none");
    assert!(env.fake.requests().is_empty());
    assert!(!complete(&env, 6));
    // Procurar de novo volta a pesquisar.
    assert_eq!(env.mal.reset_link(1).unwrap().state, "linked");
}

#[test]
fn falha_de_rede_na_procura_nao_guarda_nada() {
    let env = env(mal_server(Arc::new(Mutex::new(None)), 0));
    connect(&env);
    env.fake.set(|_| Err(NetError::Timeout));
    let status = env.mal.series_status(1, true).unwrap();
    assert_eq!(status.state, "error");
    assert!(status.error.unwrap().contains("demorou"));
    env.fake.set(mal_server(Arc::new(Mutex::new(None)), 0));
    assert_eq!(env.mal.series_status(1, true).unwrap().state, "linked");
}

#[test]
fn sem_conta_abrir_a_obra_nao_consulta_o_mal() {
    let env = env(mal_server(Arc::new(Mutex::new(None)), 0));
    assert_eq!(env.mal.series_status(1, true).unwrap().state, "unchecked");
    assert!(env.fake.requests().is_empty());
}

#[test]
fn pesquisa_manual_por_titulo_ou_endereco() {
    let (env, _) = linked_env(None, 0);
    let found = env.mal.search(1, "berserk").unwrap();
    assert_eq!(found[0].mal_id, 2);
    let direct = env.mal.search(1, "https://myanimelist.net/manga/2/Berserk").unwrap();
    assert_eq!(direct.len(), 1);
    assert!(env.fake.requests().last().unwrap().url.contains("/manga/2?"));
    assert_eq!(env.mal.search(1, "ab").unwrap_err().code(), "mal_query_short");
}

// ---------------------------------------------------------------------------
// Sincronização
// ---------------------------------------------------------------------------

#[test]
fn concluir_um_capitulo_sincroniza_e_poe_como_lendo() {
    let (env, remote) = linked_env(None, 0);
    assert!(complete(&env, 12));
    env.mal.run_queue();
    let sent = puts(&env);
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0]["num_chapters_read"], "12");
    assert_eq!(sent[0]["status"], "reading");
    assert_eq!(*lock(&remote), Some(("reading".into(), 12)));
    assert!(queue(&env).is_empty());
    let link = env.mal.series_status(1, false).unwrap().link.unwrap();
    assert_eq!(link.synced_chapters, Some(12));
    assert!(env.mal.status(&env.db()).unwrap().last_sync_at.is_some());
}

#[test]
fn abrir_ou_comecar_um_capitulo_nao_sincroniza() {
    let (env, _) = linked_env(None, 0);
    let mut conn = env.db();
    reading::acknowledge(&conn, 7).unwrap();
    reading::start(&mut conn, 7).unwrap();
    reading::touch(&conn, 7).unwrap();
    reading::end(&conn, 7).unwrap();
    // Mesmo que alguém chame a fila por engano, capítulo não concluído não entra.
    assert!(!enqueue_completion(&conn, 7).unwrap());
    assert!(queue(&env).is_empty());
    env.mal.run_queue();
    assert!(env.fake.requests().is_empty());
}

#[test]
fn nunca_diminui_o_progresso_do_mal() {
    let (env, remote) = linked_env(Some(("reading", 20)), 0);
    assert!(complete(&env, 12));
    env.mal.run_queue();
    assert!(puts(&env).is_empty(), "o MAL está mais adiantado: nada é enviado");
    assert_eq!(*lock(&remote), Some(("reading".into(), 20)));
    assert!(queue(&env).is_empty());
    // O que o MAL tinha vira a referência: capítulos anteriores nem entram na fila.
    assert!(!complete(&env, 15));
}

#[test]
fn nunca_marca_como_concluida() {
    let (env, remote) = linked_env(Some(("plan_to_read", 0)), 13);
    // A fonte tem mais capítulos que o MAL conta: vai até o total, como "lendo".
    assert!(complete(&env, 20));
    env.mal.run_queue();
    let sent = puts(&env);
    assert_eq!(sent[0]["num_chapters_read"], "13");
    assert_eq!(sent[0]["status"], "reading");
    assert!(sent.iter().all(|f| f.get("status").map(String::as_str) != Some("completed")));
    // Já concluída no MAL (pela pessoa): não é tocada.
    *lock(&remote) = Some(("completed".into(), 13));
    env.db().execute("UPDATE mal_links SET synced_chapters = NULL", []).unwrap();
    env.fake.clear();
    assert!(complete(&env, 19));
    env.mal.run_queue();
    assert!(puts(&env).is_empty());
    // Pausada ou abandonada: os capítulos sobem, a situação continua a da pessoa.
    *lock(&remote) = Some(("on_hold".into(), 2));
    env.db().execute("UPDATE mal_links SET synced_chapters = NULL", []).unwrap();
    env.fake.clear();
    assert!(complete(&env, 18));
    env.mal.run_queue();
    let sent = puts(&env);
    assert_eq!(sent[0]["num_chapters_read"], "13");
    assert!(!sent[0].contains_key("status"));
}

#[test]
fn nao_duplica_atualizacoes() {
    let (env, _) = linked_env(None, 0);
    assert!(complete(&env, 12));
    assert!(!complete(&env, 12), "o mesmo capítulo de novo");
    assert!(!complete(&env, 10), "um anterior");
    assert!(complete(&env, 13), "um seguinte só aumenta o alvo");
    assert_eq!(queue(&env), vec![(13, "pending".into(), 0)]);
    env.mal.run_queue();
    assert_eq!(puts(&env).len(), 1);
    assert_eq!(puts(&env)[0]["num_chapters_read"], "13");
    env.fake.clear();
    assert!(!complete(&env, 13), "já confirmado no MAL");
    env.mal.run_queue();
    assert!(env.fake.requests().is_empty());
}

#[test]
fn a_fila_sobrevive_a_fechar_e_abrir_o_app() {
    let (env, remote) = linked_env(None, 0);
    env.fake.set(|_| Err(NetError::Network("sem rede".into())));
    assert!(complete(&env, 8));
    env.mal.run_queue();
    assert_eq!(queue(&env), vec![(8, "pending".into(), 1)]);
    let path = env.mal.db_path.clone();

    // "Reabre": outro serviço sobre o mesmo banco, com a rede de volta.
    let fake = Fake::new(mal_server(remote.clone(), 0));
    let mut reopened = Mal::new(path, Some("cliente".into()), Box::new(fake.clone()), Box::new(TestVault), Arc::new(|_, _| {}));
    reopened.spacing = Duration::ZERO;
    let status = reopened.status(&env.db()).unwrap();
    assert_eq!(status.pending, 1);
    assert!(status.pending_error.as_deref().unwrap().contains("conexão"));
    env.db().execute("UPDATE mal_sync_queue SET next_attempt_at = 0", []).unwrap();
    reopened.run_queue();
    assert!(queue(&env).is_empty());
    assert_eq!(*lock(&remote), Some(("reading".into(), 8)));
}

#[test]
fn falhas_temporarias_tentam_de_novo_com_espera_e_depois_aparecem() {
    let (env, _) = linked_env(None, 0);
    env.fake.set(|_| status(503));
    assert!(complete(&env, 8));
    let wait = env.mal.run_queue();
    assert!(wait >= Duration::from_secs(1), "não insiste na hora");
    let next: i64 = env.db().query_row("SELECT next_attempt_at FROM mal_sync_queue", [], |r| r.get(0)).unwrap();
    assert!(next > now_ms() + 30_000);
    for _ in 0..MAX_ATTEMPTS {
        env.db().execute("UPDATE mal_sync_queue SET next_attempt_at = 0", []).unwrap();
        env.mal.run_queue();
    }
    let status = env.mal.status(&env.db()).unwrap();
    assert_eq!(status.failures.len(), 1);
    assert!(status.failures[0].error.as_deref().unwrap().contains("503"));
    assert_eq!(status.failures[0].chapters, 8);

    // "Tentar de novo" com o MAL de volta.
    env.fake.set(mal_server(Arc::new(Mutex::new(None)), 0));
    env.mal.retry(&env.db()).unwrap();
    env.mal.run_queue();
    assert!(queue(&env).is_empty());
}

#[test]
fn obra_removida_do_mal_falha_sem_insistir() {
    let (env, _) = linked_env(None, 0);
    env.fake.set(|_| status(404));
    assert!(complete(&env, 8));
    env.mal.run_queue();
    assert_eq!(queue(&env), vec![(8, "failed".into(), 1)]);
    assert_eq!(env.mal.discard_failed(&env.db()).unwrap(), 1);
}

#[test]
fn capitulo_sem_numero_nao_vai_para_a_fila() {
    let (env, _) = linked_env(None, 0);
    env.db().execute("UPDATE online_chapters SET number = NULL, title = 'Especial de Natal' WHERE id = 9", []).unwrap();
    assert!(!complete(&env, 9));
}

#[test]
fn plano_da_sincronizacao() {
    let s = |status: Option<&str>, read: i64, rereading: bool| ListStatus { status: status.map(str::to_string), num_chapters_read: read, is_rereading: rereading };
    assert_eq!(plan(5, 0, None), Plan::Update { chapters: 5, status: Some("reading") });
    assert_eq!(plan(5, 0, Some(&s(Some("reading"), 7, false))), Plan::Skip { remote: 7 });
    assert_eq!(plan(5, 0, Some(&s(Some("reading"), 5, false))), Plan::Skip { remote: 5 });
    assert_eq!(plan(9, 0, Some(&s(Some("dropped"), 5, false))), Plan::Update { chapters: 9, status: None });
    assert_eq!(plan(30, 20, Some(&s(Some("reading"), 20, false))), Plan::Skip { remote: 20 });
    assert_eq!(plan(9, 0, Some(&s(Some("completed"), 3, false))), Plan::Skip { remote: 3 });
    assert_eq!(plan(9, 0, Some(&s(Some("completed"), 3, true))), Plan::Update { chapters: 9, status: None }, "relendo, conta");
}
