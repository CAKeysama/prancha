use std::collections::BTreeMap;
use std::sync::Arc;

use rusqlite::params;
use serde_json::{Value, json};

use super::*;
use crate::extensions::runtime::Chapter;
use crate::extensions::sources;

/// Nuvem falsa com as mesmas regras de conflito do banco (supabase/migrations/20261115220000_sync.sql), em memória.
#[derive(Default)]
struct FakeCloud {
    state: Mutex<Cloud>,
}

#[derive(Default)]
struct Cloud {
    seq: i64,
    rows: BTreeMap<String, (i64, Value)>,
    fail: bool,
    pushes: usize,
}

fn row_key(c: &Value) -> String {
    let t = c["t"].as_str().unwrap();
    match t {
        "repository" => format!("repository|{}", c["url"].as_str().unwrap()),
        "extension" | "collection" => format!("{t}|{}", c["id"].as_str().unwrap()),
        "series" => format!("series|{}", c["key"].as_str().unwrap()),
        "chapter" => format!("chapter|{}|{}", c["series"].as_str().unwrap(), c["key"].as_str().unwrap()),
        "series_collection" => format!("series_collection|{}|{}", c["series"].as_str().unwrap(), c["collection"].as_str().unwrap()),
        _ => format!("mal_link|{}", c["series"].as_str().unwrap()),
    }
}

fn newer(change: &Value, old: &Value, at: &str) -> bool {
    change[at].as_i64().unwrap_or(0) > old[at].as_i64().unwrap_or(0)
}

impl Cloud {
    fn series_alive(&self, key: &str) -> bool {
        self.rows.get(&format!("series|{key}")).is_some_and(|(_, s)| s["removed"] != json!(true))
    }

    fn apply(&mut self, c: &Value) {
        let key = row_key(c);
        let old = self.rows.get(&key).map(|(_, v)| v.clone());
        let merged = match c["t"].as_str().unwrap() {
            "series" => {
                let mut row = old.unwrap_or_else(|| json!({ "following": false, "favorite": false, "chapter_sort": "desc", "removed": false }));
                for (field, at) in [("title", "meta_at"), ("following", "following_at"), ("favorite", "favorite_at"), ("chapter_sort", "chapter_sort_at"), ("reader_mode", "reader_mode_at"), ("removed", "removed_at")] {
                    if newer(c, &row, at) {
                        row[field] = c[field].clone();
                        row[at] = c[at].clone();
                        if field == "title" {
                            row["cover_url"] = c["cover_url"].clone();
                        }
                    }
                }
                for field in ["t", "key", "extension_id", "source_id", "item_key"] {
                    row[field] = c[field].clone();
                }
                if row["removed"] == json!(true) {
                    let series = c["key"].as_str().unwrap().to_string();
                    self.rows.retain(|k, _| !(k.starts_with(&format!("chapter|{series}|")) || k.starts_with(&format!("series_collection|{series}|"))));
                }
                row
            }
            "chapter" => {
                if !self.series_alive(c["series"].as_str().unwrap()) {
                    return;
                }
                let mut row = old.unwrap_or_else(|| json!({ "t": "chapter", "series": c["series"], "key": c["key"] }));
                for field in ["started_at", "acknowledged_at"] {
                    if let Some(v) = c[field].as_i64() {
                        row[field] = json!(row[field].as_i64().map_or(v, |o| o.min(v)));
                    }
                }
                if c.get("read").is_some() {
                    if row["read"] == json!(true) && c["read"] == json!(true) {
                        row["completed_at"] = json!(row["completed_at"].as_i64().unwrap().min(c["completed_at"].as_i64().unwrap_or(i64::MAX)));
                    } else if newer(c, &row, "read_at") {
                        row["completed_at"] = if c["read"] == json!(true) { json!(c["completed_at"].as_i64().or(c["read_at"].as_i64())) } else { Value::Null };
                    }
                    if newer(c, &row, "read_at") {
                        row["read"] = c["read"].clone();
                        row["read_at"] = c["read_at"].clone();
                    }
                }
                if c.get("page_index").is_some() && newer(c, &row, "position_at") {
                    for field in ["page_index", "page_count", "scroll_ratio", "position_at"] {
                        row[field] = c[field].clone();
                    }
                }
                row
            }
            "series_collection" | "mal_link" if !self.series_alive(c["series"].as_str().unwrap()) => return,
            _ => match old {
                Some(old) if !newer(c, &old, "at") => return,
                _ => c.clone(),
            },
        };
        self.seq += 1;
        self.rows.insert(key, (self.seq, merged));
    }
}

impl Remote for FakeCloud {
    fn push(&self, device: &Value, changes: &[Value]) -> Result<Value, CallError> {
        let mut cloud = self.state.lock().unwrap();
        if cloud.fail {
            return Err(CallError::Api(crate::account::api::ApiError::Server(500)));
        }
        assert!(device["id"].as_str().is_some_and(|id| id.len() == 36), "o aparelho vai com o id");
        assert!(changes.len() <= MAX_BATCH);
        cloud.pushes += 1;
        for change in changes {
            cloud.apply(change);
        }
        Ok(json!({ "applied": changes.len(), "skipped": 0, "cursor": cloud.seq }))
    }

    fn pull(&self, since: i64, limit: u32) -> Result<Value, CallError> {
        let cloud = self.state.lock().unwrap();
        let mut rows: Vec<&(i64, Value)> = cloud.rows.values().filter(|(seq, _)| *seq > since).collect();
        rows.sort_by_key(|(seq, _)| *seq);
        let more = rows.len() > limit as usize;
        rows.truncate(limit as usize);
        let next = rows.last().map_or(since, |(seq, _)| *seq);
        Ok(json!({ "changes": rows.iter().map(|(_, v)| v.clone()).collect::<Vec<_>>(), "next": next, "more": more, "reset": false, "server_time": now_ms() }))
    }
}

const EXTENSION: &str = "dev.prancha.demo";

/// Um aparelho com a extensão do Prancha instalada e uma obra com três capítulos.
fn device() -> (tempfile::TempDir, Core, i64) {
    let dir = tempfile::tempdir().unwrap();
    let core = Core::init(dir.path().to_path_buf(), Arc::new(|_, _| {})).unwrap();
    let id = {
        let conn = core.db();
        conn.execute(
            "INSERT INTO installed_extensions (id, repository_id, repository_name, public_key, key_fingerprint, name, version, entry, file_name, sha256, installed_at, updated_at)
             VALUES (?1, 'r', 'R', 'k', 'f', 'Demo', '1.0.0', '{\"runtime\":\"wasm-component\"}', 'e', 'h', 1, 1)",
            [EXTENSION],
        )
        .unwrap();
        // Entrar na conta é o que liga o envio: aqui, direto no banco.
        conn.execute("INSERT INTO sync_account (id, user_id, provider, secret, expires_at, signed_in_at) VALUES (1, 'u1', 'google', x'00', 0, 0)", []).unwrap();
        conn.execute("INSERT INTO online_series (extension_id, source_id, remote_id, title, created_at, updated_at) VALUES (?1, 'fonte', 'obra-1', 'Obra', 1000, 1000)", [EXTENSION]).unwrap();
        conn.last_insert_rowid()
    };
    list_chapters(&core, id);
    (dir, core, id)
}

fn list_chapters(core: &Core, series_id: i64) {
    let chapters: Vec<Chapter> = (1..=3).map(|n| Chapter { id: format!("cap-{n}"), title: format!("Capítulo {n}"), number: Some(n.to_string()), published_at: None, order: n }).collect();
    sources::store_chapters(&mut core.db(), series_id, &chapters).unwrap();
}

fn chapter(core: &Core, series_id: i64, remote: &str) -> i64 {
    core.db().query_row("SELECT id FROM online_chapters WHERE series_id = ?1 AND remote_id = ?2", params![series_id, remote], |r| r.get(0)).unwrap()
}

fn sync(core: &Core, cloud: &FakeCloud) -> Report {
    let mut conn = db::open(&core.paths.db).unwrap();
    run(core, cloud, &mut conn).unwrap()
}

fn series_of(core: &Core) -> Option<i64> {
    core.db().query_row("SELECT id FROM online_series WHERE remote_id = 'obra-1'", [], |r| r.get(0)).ok()
}

#[test]
fn as_chaves_sao_estaveis_e_portateis() {
    let key = series_key(EXTENSION, "fonte", "obra-1");
    assert_eq!(key.len(), 32);
    assert!(key.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
    assert_eq!(key, series_key(EXTENSION, "fonte", "obra-1"));
    assert_ne!(key, series_key(EXTENSION, "fonte", "obra-2"));
    // O separador impede que "a" + "bc" e "ab" + "c" virem a mesma obra.
    assert_ne!(series_key("a", "bc", "d"), series_key("ab", "c", "d"));
    assert_ne!(chapter_key(&key, "cap-1"), chapter_key(&series_key(EXTENSION, "fonte", "obra-2"), "cap-1"));
}

#[test]
fn a_leitura_do_pc_continua_no_celular() {
    let cloud = FakeCloud::default();
    let (_a, pc, series) = device();
    let (first, second) = (chapter(&pc, series, "cap-1"), chapter(&pc, series, "cap-2"));
    {
        let mut conn = pc.db();
        sources::set_favorite(&conn, series, true).unwrap();
        crate::extensions::reading::complete(&mut conn, first).unwrap();
        sources::save_chapter_progress(&conn, second, 7, 20, 0.5).unwrap();
        let folder = crate::library::create_collection(&conn, "Lendo").unwrap();
        sources::add_series_to_collection(&mut conn, folder, &[series]).unwrap();
    }
    let sent = sync(&pc, &cloud);
    assert!(sent.pushed >= 4, "obra, capítulos, pasta e a obra na pasta: {sent:?}");

    // O celular não tinha nada: a obra aparece, e o estado entra assim que a lista de capítulos chega.
    let dir = tempfile::tempdir().unwrap();
    let phone = Core::init(dir.path().to_path_buf(), Arc::new(|_, _| {})).unwrap();
    phone.db().execute("INSERT INTO sync_account (id, user_id, provider, secret, expires_at, signed_in_at) VALUES (1, 'u1', 'google', x'00', 0, 0)", []).unwrap();
    let got = sync(&phone, &cloud);
    assert!(got.applied);
    let id = series_of(&phone).expect("a obra veio da nuvem");
    let (favorite, title): (bool, String) = phone.db().query_row("SELECT favorite, title FROM online_series WHERE id = ?1", [id], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!((favorite, title.as_str()), (true, "Obra"));
    let folders: Vec<String> = {
        let conn = phone.db();
        let mut s = conn.prepare("SELECT c.name FROM series_collections x JOIN collections c ON c.id = x.collection_id WHERE x.series_id = ?1").unwrap();
        s.query_map([id], |r| r.get(0)).unwrap().collect::<Result<_, _>>().unwrap()
    };
    assert_eq!(folders, vec!["Lendo".to_string()]);
    assert_eq!(phone.db().query_row("SELECT value FROM settings WHERE key = 'plugins.online'", [], |r| r.get::<_, String>(0)).unwrap(), "true");

    list_chapters(&phone, id);
    let (one, two) = (chapter(&phone, id, "cap-1"), chapter(&phone, id, "cap-2"));
    let (completed, unread): (Option<i64>, bool) =
        phone.db().query_row("SELECT completed_at, manual_unread FROM online_chapters WHERE id = ?1", [one], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert!(completed.is_some() && !unread, "o capítulo 1 chegou lido");
    let position: (i64, i64) = phone.db().query_row("SELECT page_index, page_count FROM chapter_progress WHERE chapter_id = ?1", [two], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(position, (7, 20));
    let listed = sources::library_series(&phone.db()).unwrap();
    assert!(listed.iter().any(|s| s.id == id));

    // Aplicar o que veio da nuvem não volta para a fila de envio.
    let queued: i64 = phone.db().query_row("SELECT count(*) FROM sync_dirty", [], |r| r.get(0)).unwrap();
    assert_eq!(queued, 0);
}

#[test]
fn a_mudanca_mais_nova_vence_e_a_remocao_chega_ao_outro_aparelho() {
    let cloud = FakeCloud::default();
    let (_a, pc, series) = device();
    let (_b, phone, phone_series) = device();
    sources::set_favorite(&pc.db(), series, true).unwrap();
    sources::set_following(&pc.db(), series, true).unwrap();
    sync(&pc, &cloud);
    sync(&phone, &cloud);
    assert!(phone.db().query_row("SELECT favorite FROM online_series WHERE id = ?1", [phone_series], |r| r.get::<_, bool>(0)).unwrap());

    // O celular desfaz o favorito depois: vale no PC.
    std::thread::sleep(Duration::from_millis(5));
    sources::set_favorite(&phone.db(), phone_series, false).unwrap();
    sync(&phone, &cloud);
    sync(&pc, &cloud);
    let (favorite, following): (bool, bool) = pc.db().query_row("SELECT favorite, following FROM online_series WHERE id = ?1", [series], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!((favorite, following), (false, true), "cada campo com a própria data");

    // O PC tira a obra da biblioteca: some do celular também.
    std::thread::sleep(Duration::from_millis(5));
    sources::forget_series(&pc, series).unwrap();
    sync(&pc, &cloud);
    sync(&phone, &cloud);
    let listed = sources::library_series(&phone.db()).unwrap();
    assert!(series_of(&phone).is_none_or(|id| !listed.iter().any(|s| s.id == id)));
    let gone = cloud.state.lock().unwrap().rows.get(&format!("series|{}", series_key(EXTENSION, "fonte", "obra-1"))).unwrap().1["removed"].clone();
    assert_eq!(gone, json!(true));
}

#[test]
fn uma_falha_no_envio_mantem_a_fila_e_o_erro() {
    let cloud = FakeCloud::default();
    let (_a, pc, series) = device();
    sync(&pc, &cloud);
    sources::set_favorite(&pc.db(), series, true).unwrap();
    struct PushFails<'a>(&'a FakeCloud);
    impl Remote for PushFails<'_> {
        fn push(&self, _: &Value, _: &[Value]) -> Result<Value, CallError> {
            Err(CallError::Api(crate::account::api::ApiError::Server(500)))
        }
        fn pull(&self, since: i64, limit: u32) -> Result<Value, CallError> {
            self.0.pull(since, limit)
        }
    }
    let mut conn = db::open(&pc.paths.db).unwrap();
    assert!(run(&pc, &PushFails(&cloud), &mut conn).is_err());
    let (queued, error): (i64, Option<String>) = conn.query_row("SELECT (SELECT count(*) FROM sync_dirty), last_error FROM sync_state", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!(queued, 1);
    assert!(error.is_some());
    // Na próxima, sobe.
    sync(&pc, &cloud);
    let key = format!("series|{}", series_key(EXTENSION, "fonte", "obra-1"));
    assert_eq!(cloud.state.lock().unwrap().rows.get(&key).unwrap().1["favorite"], json!(true));
}

#[test]
fn obras_do_mihon_e_hqs_locais_nao_sobem() {
    let cloud = FakeCloud::default();
    let (_a, pc, _) = device();
    {
        let conn = pc.db();
        conn.execute(
            "INSERT INTO installed_extensions (id, repository_id, repository_name, public_key, key_fingerprint, name, version, entry, file_name, sha256, installed_at, updated_at)
             VALUES ('mihon.ext', 'r', 'R', 'k', 'f', 'Mihon', '1.0.0', ?1, 'e', 'h', 1, 1)",
            [json!({ "runtime": crate::extensions::suwayomi::RUNTIME }).to_string()],
        )
        .unwrap();
        conn.execute("INSERT INTO online_series (extension_id, source_id, remote_id, title, favorite, created_at, updated_at) VALUES ('mihon.ext', '123', '45', 'Do Mihon', 1, 1, 1)", []).unwrap();
    }
    sync(&pc, &cloud);
    let cloud = cloud.state.lock().unwrap();
    assert!(cloud.rows.values().all(|(_, v)| v["extension_id"] != json!("mihon.ext")));
    assert!(cloud.rows.values().all(|(_, v)| !v.to_string().contains("Do Mihon")));
}

#[test]
fn lotes_respeitam_os_limites_da_nuvem() {
    let changes: Vec<Value> = (0..1200).map(|i| json!({ "t": "collection", "id": format!("{i}"), "name": "x".repeat(100) })).collect();
    let batches = push::batches(changes);
    assert_eq!(batches.iter().map(Vec::len).sum::<usize>(), 1200);
    assert!(batches.iter().all(|b| b.len() <= MAX_BATCH && serde_json::to_string(b).unwrap().len() <= MAX_BATCH_BYTES + 2));
    assert_eq!(batches[0][0]["id"], json!("0"), "a ordem fica");
}
