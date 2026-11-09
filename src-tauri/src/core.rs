//! Estado compartilhado do aplicativo (independente do Tauri, para facilitar testes).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use rusqlite::Connection;
use serde_json::json;

use crate::db;
use crate::error::{AppError, AppResult};
use crate::extensions::downloads::Downloads;
use crate::extensions::net::Net;
use crate::extensions::runtime::SourceEngine;
use crate::extensions::sources::{ChapterPages, Gate, Requests};
use crate::extensions::suwayomi::Suwayomi;
use crate::import::{EVENT_LIBRARY, Emit, Importer};
use crate::library;
use crate::notifications::{Event, Kind, NewNotification};
use crate::pages::PageService;
use crate::telemetry::Telemetry;

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub db: PathBuf,
    pub covers: PathBuf,
    pub pages_cache: PathBuf,
}

impl AppPaths {
    pub fn new(data_dir: PathBuf) -> std::io::Result<Self> {
        let p = AppPaths {
            db: data_dir.join("prancha.db"),
            covers: data_dir.join("cache").join("covers"),
            pages_cache: data_dir.join("cache").join("pages"),
            data_dir,
        };
        std::fs::create_dir_all(&p.covers)?;
        std::fs::create_dir_all(&p.pages_cache)?;
        Ok(p)
    }
}

pub struct Core {
    pub paths: AppPaths,
    db: Mutex<Connection>,
    pub pages: PageService,
    pub importer: Importer,
    pub emit: Emit,
    pub guided_lock: Mutex<()>,
    pub net: Arc<Net>,
    /// Criado no primeiro uso: quem não usa extensões não paga por ele e nada aqui impede o app de abrir.
    engine: OnceLock<Result<SourceEngine, String>>,
    pub downloads: Downloads,
    pub requests: Requests,
    pub cover_gate: Gate,
    /// Separada das capas: a página que a pessoa está lendo não espera uma grade de capas.
    pub page_gate: Gate,
    pub chapter_pages: ChapterPages,
    /// Também só no primeiro uso: sem repositório do Mihon, nada é baixado nem iniciado.
    pub suwayomi: Suwayomi,
    /// Notificações do sistema operacional, ligadas pelo app Tauri. Sem ele (testes), só a central.
    pub notifier: OnceLock<crate::notifications::Notifier>,
    /// Desligada até a pessoa aceitar. Ver telemetry.rs.
    pub telemetry: Arc<Telemetry>,
}

impl Core {
    pub fn init(data_dir: PathBuf, emit: Emit) -> AppResult<Self> {
        let paths = AppPaths::new(data_dir)?;
        let conn = db::open(&paths.db)?;
        library::mark_interrupted_jobs(&conn)?;
        // Nenhuma sessão de leitura sobrevive a uma abertura do app: as que uma queda deixou abertas viram "Em andamento".
        crate::extensions::reading::recover_sessions(&conn)?;
        if let Err(e) = crate::notifications::prune(&conn, db::now_ms()) {
            log::warn!("limpeza das notificações falhou: {e}");
        }
        let telemetry = Arc::new(Telemetry::from_env());
        // A telemetria nunca impede o app de abrir: se o estado dela não puder ser lido, fica desligada.
        if let Err(e) = telemetry.load(&conn) {
            log::warn!("estado da telemetria não lido: {e}");
        }
        let db_path = paths.db.clone();
        let importer = Importer::spawn(move || db::open(&db_path), paths.covers.clone(), emit.clone())?;
        // Capas pendentes ou geradas com parâmetros antigos são refeitas em segundo plano.
        importer.enqueue_covers();
        Ok(Core { pages: PageService::new(paths.pages_cache.clone()), paths, db: Mutex::new(conn), importer, emit, guided_lock: Mutex::new(()), net: Arc::new(Net::from_env()), engine: OnceLock::new(), downloads: Downloads::default(), requests: Requests::default(), cover_gate: Gate::default(), page_gate: Gate::default(), chapter_pages: ChapterPages::default(), suwayomi: Suwayomi::default(), notifier: OnceLock::new(), telemetry })
    }

    pub fn db(&self) -> MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn engine(&self) -> AppResult<&SourceEngine> {
        let built = self.engine.get_or_init(|| SourceEngine::new(self.net.clone(), self.paths.db.clone()).map_err(|e| e.to_string()));
        built.as_ref().map_err(|e| AppError::user("extension_load", e.clone()))
    }

    /// Confere se os arquivos ainda existem e atualiza o indicador `missing`.
    /// Usa uma conexão própria para não segurar o banco principal.
    pub fn verify_files(&self) -> AppResult<(usize, usize)> {
        let conn = db::open(&self.paths.db)?;
        let infos = library::all_file_infos(&conn)?;
        let (mut missing, mut restored) = (0, 0);
        let started = db::now_ms();
        let mut gone = NewNotification::new(Kind::FilesMissing).grouped(crate::notifications::DOWNLOADS_WINDOW_MS);
        for info in infos {
            let exists = Path::new(&info.path).exists();
            if !exists && !info.missing {
                library::set_missing(&conn, info.id, true)?;
                missing += 1;
                // A chave é a HQ e o momento em que o arquivo sumiu: se voltar e sumir de novo, é outro aviso.
                gone = gone.event(Event::new(format!("library:missing:{}:{started}", info.id)).item(library::comic_title(&conn, info.id).unwrap_or_default()));
                if missing == 1 {
                    gone = gone.comic(info.id);
                }
            } else if exists && info.missing {
                library::set_missing(&conn, info.id, false)?;
                restored += 1;
            }
        }
        if missing + restored > 0 {
            (self.emit)(EVENT_LIBRARY, json!({}));
        }
        if missing > 0 {
            crate::notifications::publish_with(self, &conn, &gone);
        }
        Ok((missing, restored))
    }
}
