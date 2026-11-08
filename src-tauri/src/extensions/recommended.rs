//! Repositórios recomendados: uma lista curta publicada no repositório do Prancha (`extensions/recomendados.json`).
//! Só entram repositórios que o Prancha adiciona e usa hoje: toda entrada tem o endereço para adicionar.
//! A lista só descreve. Não traz código, não muda configuração e não adiciona nada sozinha:
//! adicionar um recomendado passa pela mesma revisão de chave de um endereço digitado.

use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::Url;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::net::{Access, Limits, Net, Request};
use super::repository::{self, is_valid_id, short_text};
use crate::core::Core;
use crate::db::now_ms;
use crate::error::AppResult;

/// A cópia que acompanha o aplicativo: vale enquanto não houver uma baixada.
const BUNDLED: &str = include_str!("../../../extensions/recomendados.json");
const REMOTE: &str = "https://raw.githubusercontent.com/CAKeysama/prancha/main/extensions/recomendados.json";
const CACHE_FILE: &str = "recomendados.json";
const MAX_AGE_MS: i64 = 24 * 60 * 60 * 1000;
const LIMITS: Limits = Limits { max_bytes: 64 * 1024, timeout: Duration::from_secs(10) };
const MAX_ENTRIES: usize = 30;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recommended {
    pub id: String,
    pub name: String,
    pub description: String,
    pub url: String,
    pub note: Option<String>,
    /// Já está entre os repositórios da pessoa.
    pub added: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Origin {
    Remote,
    Cache,
    Bundled,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecommendedList {
    pub entries: Vec<Recommended>,
    pub origin: Origin,
    /// Quando a cópia salva foi baixada; `None` na embutida.
    pub fetched_at: Option<i64>,
    /// Vale consultar a lista publicada: a tela faz isso em segundo plano.
    pub stale: bool,
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Cache {
    fetched_at: i64,
    body: String,
}

/// Sem `refresh`, responde na hora com a cópia salva ou a embutida. Com `refresh`, consulta a publicada.
pub fn load(core: &Core, refresh: bool) -> AppResult<RecommendedList> {
    let path = cache_path(core);
    let mut list = if refresh { refreshed(&path, &core.net, fetch(&core.net)) } else { local(&path, &core.net) };
    let stored: Vec<String> = repository::list(&core.db())?.into_iter().map(|r| r.url).collect();
    mark_added(&mut list.entries, &stored);
    Ok(list)
}

fn cache_path(core: &Core) -> PathBuf {
    core.paths.data_dir.join(CACHE_FILE)
}

fn fetch(net: &Net) -> Result<String, String> {
    let response = net.get(REMOTE, Access::Public, LIMITS, Request::default()).map_err(|e| e.to_string())?;
    if !response.is_success() {
        return Err(format!("resposta {}", response.status));
    }
    String::from_utf8(response.body).map_err(|_| "a lista não é texto UTF-8".into())
}

/// Uma lista publicada inválida não substitui a que já funciona.
fn refreshed(path: &Path, net: &Net, fetched: Result<String, String>) -> RecommendedList {
    match fetched.and_then(|body| parse(&body, net).map(|entries| (body, entries))) {
        Ok((body, entries)) => {
            let now = now_ms();
            if let Err(e) = write_cache(path, now, &body) {
                log::warn!("recomendados: a lista não foi salva ({e})");
            }
            RecommendedList { entries, origin: Origin::Remote, fetched_at: Some(now), stale: false, error: None }
        }
        Err(why) => {
            log::warn!("recomendados: {why}");
            RecommendedList { stale: false, error: Some("Não foi possível consultar a lista mais recente agora.".into()), ..local(path, net) }
        }
    }
}

fn local(path: &Path, net: &Net) -> RecommendedList {
    match read_cache(path, net) {
        Some((fetched_at, entries)) => {
            let stale = !(0..=MAX_AGE_MS).contains(&(now_ms() - fetched_at));
            RecommendedList { entries, origin: Origin::Cache, fetched_at: Some(fetched_at), stale, error: None }
        }
        // A embutida é conferida pelos testes; vazia, a tela só não mostra recomendados.
        None => RecommendedList { entries: parse(BUNDLED, net).unwrap_or_default(), origin: Origin::Bundled, fetched_at: None, stale: true, error: None },
    }
}

fn read_cache(path: &Path, net: &Net) -> Option<(i64, Vec<Recommended>)> {
    let cache: Cache = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    Some((cache.fetched_at, parse(&cache.body, net).ok()?))
}

/// Grava ao lado e troca de uma vez: uma queda no meio não deixa uma cópia pela metade.
fn write_cache(path: &Path, fetched_at: i64, body: &str) -> std::io::Result<()> {
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, serde_json::to_vec(&Cache { fetched_at, body: body.to_string() })?)?;
    std::fs::rename(&temporary, path)
}

/// Lê a lista e descarta, uma a uma, as entradas fora das regras.
fn parse(body: &str, net: &Net) -> Result<Vec<Recommended>, String> {
    let value: Value = serde_json::from_str(body).map_err(|_| "a lista não é um JSON válido".to_string())?;
    if value.get("schemaVersion").and_then(Value::as_u64) != Some(1) {
        return Err("versão desconhecida da lista".into());
    }
    let items = value.get("repositories").and_then(Value::as_array).ok_or("a lista não tem repositórios")?;
    let mut entries: Vec<Recommended> = Vec::new();
    for item in items {
        if entries.len() == MAX_ENTRIES {
            break;
        }
        if let Some(entry) = entry(item, net).filter(|e| entries.iter().all(|other| other.id != e.id)) {
            entries.push(entry);
        }
    }
    if entries.is_empty() {
        return Err("nenhuma entrada válida na lista".into());
    }
    Ok(entries)
}

fn entry(item: &Value, net: &Net) -> Option<Recommended> {
    let text = |value: &Value, limit: usize| value.as_str().map(str::trim).filter(|t| short_text(t, limit)).map(str::to_string);
    let id = item.get("id")?.as_str().filter(|id| id.len() <= 64 && is_valid_id(id))?.to_string();
    let name = text(item.get("name")?, 60)?;
    let description = text(item.get("description")?, 240)?;
    let url = public_https(item.get("url")?.as_str()?, net)?;
    // Ausente vale `None`; presente e inválida descarta a entrada.
    let note = match item.get("note") {
        None | Some(Value::Null) => None,
        Some(value) => Some(text(value, 240)?),
    };
    Some(Recommended { id, name, description, url, note, added: false })
}

fn public_https(address: &str, net: &Net) -> Option<String> {
    let url = Url::parse(address).ok().filter(|url| address.len() <= 512 && url.scheme() == "https")?;
    net.check(&url, Access::Public).ok().map(|()| url.into())
}

/// Mesma pasta: quem cola `…/index.pb` fica gravado como `…/repo.json`.
fn folder(address: &str) -> Option<String> {
    let url = Url::parse(address).ok()?;
    let path = url.path();
    Some(format!("{}://{}{}", url.scheme(), url.host_str()?, &path[..=path.rfind('/')?]))
}

fn mark_added(entries: &mut [Recommended], stored: &[String]) {
    let stored: Vec<String> = stored.iter().filter_map(|url| folder(url)).collect();
    for entry in entries {
        entry.added = folder(&entry.url).is_some_and(|f| stored.contains(&f));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn net() -> Net {
        Net::new(false)
    }

    fn catalog(repositories: Value) -> String {
        serde_json::json!({ "schemaVersion": 1, "repositories": repositories }).to_string()
    }

    fn compatible(id: &str, url: &str) -> Value {
        serde_json::json!({ "id": id, "name": "Nome", "description": "Descrição", "url": url })
    }

    #[test]
    fn every_bundled_entry_is_valid() {
        let entries = parse(BUNDLED, &net()).unwrap();
        assert_eq!(entries.len(), serde_json::from_str::<Value>(BUNDLED).unwrap()["repositories"].as_array().unwrap().len(), "nenhuma entrada embutida pode ser descartada");
        assert!(entries.iter().any(|e| e.id == "keiyoushi" && e.url == "https://github.com/keiyoushi/extensions/raw/repo/index.pb"));
    }

    #[test]
    fn malformed_entries_are_dropped_one_by_one() {
        let body = catalog(serde_json::json!([
            compatible("ok", "https://exemplo.com/prancha-repo.json"),
            compatible("ok", "https://outro.com/prancha-repo.json"),
            compatible("Maiúsculo", "https://exemplo.com/a.json"),
            compatible("http", "http://exemplo.com/prancha-repo.json"),
            compatible("local", "https://192.168.0.10/prancha-repo.json"),
            compatible("porta", "https://exemplo.com:8443/prancha-repo.json"),
            compatible("sem-url", "nao é endereço"),
            { "id": "sem-endereco", "name": "Nome", "description": "Descrição" },
            { "id": "nome-longo", "name": "n".repeat(61), "description": "Descrição", "url": "https://exemplo.com/x.json" },
            { "id": "controle", "name": "Nome\u{0007}", "description": "Descrição", "url": "https://exemplo.com/x.json" },
            { "id": "nota-ruim", "name": "Nome", "description": "Descrição", "url": "https://exemplo.com/x.json", "note": 3 },
            { "id": "com-nota", "name": "Nome", "description": "Descrição", "url": "https://exemplo.com/x.json", "note": "Aviso" },
            "não é objeto",
        ]));
        let entries = parse(&body, &net()).unwrap();
        let ids: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["ok", "com-nota"]);
        assert_eq!(entries[0].url, "https://exemplo.com/prancha-repo.json", "o primeiro com o mesmo id vale");
        assert_eq!((entries[0].note.as_deref(), entries[1].note.as_deref()), (None, Some("Aviso")));
    }

    #[test]
    fn a_broken_or_unknown_list_is_refused_whole() {
        assert!(parse("{", &net()).is_err());
        assert!(parse(&serde_json::json!({ "schemaVersion": 2, "repositories": [compatible("a", "https://exemplo.com/r.json")] }).to_string(), &net()).is_err());
        assert!(parse(&catalog(serde_json::json!([])), &net()).is_err());
        assert!(parse(&catalog(serde_json::json!({ "a": 1 })), &net()).is_err());
    }

    #[test]
    fn the_list_is_capped() {
        let many: Vec<Value> = (0..40).map(|i| compatible(&format!("r{i}"), "https://exemplo.com/r.json")).collect();
        assert_eq!(parse(&catalog(Value::Array(many)), &net()).unwrap().len(), MAX_ENTRIES);
    }

    #[test]
    fn added_compares_the_folder_of_the_address() {
        let mut entries = parse(BUNDLED, &net()).unwrap();
        mark_added(&mut entries, &["https://github.com/keiyoushi/extensions/raw/repo/repo.json".into(), "https://exemplo.com/outro/prancha-repo.json".into()]);
        let added: Vec<&str> = entries.iter().filter(|e| e.added).map(|e| e.id.as_str()).collect();
        assert_eq!(added, ["keiyoushi"]);
    }

    #[test]
    fn a_failed_refresh_keeps_the_saved_copy_and_a_good_one_replaces_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CACHE_FILE);
        let net = net();

        let first = local(&path, &net);
        assert_eq!((first.origin, first.stale), (Origin::Bundled, true));

        let body = catalog(serde_json::json!([compatible("novo", "https://exemplo.com/r.json")]));
        let fresh = refreshed(&path, &net, Ok(body));
        assert_eq!((fresh.origin, fresh.error.is_none()), (Origin::Remote, true));
        let saved = local(&path, &net);
        assert_eq!((saved.origin, saved.stale, saved.entries[0].id.as_str()), (Origin::Cache, false, "novo"));

        for failure in [Err("sem rede".to_string()), Ok("{\"schemaVersion\": 1, \"repositories\": [\"lixo\"]}".to_string())] {
            let kept = refreshed(&path, &net, failure);
            assert_eq!((kept.origin, kept.entries[0].id.as_str(), kept.stale), (Origin::Cache, "novo", false));
            assert!(kept.error.is_some());
        }

        // Uma cópia antiga continua valendo, mas pede a nova.
        write_cache(&path, now_ms() - MAX_AGE_MS - 1, &catalog(serde_json::json!([compatible("velho", "https://exemplo.com/r.json")]))).unwrap();
        assert!(local(&path, &net).stale);

        std::fs::write(&path, b"corrompido").unwrap();
        assert_eq!(local(&path, &net).origin, Origin::Bundled);
    }
}
