//! Estado compartilhado do aplicativo (independente do Tauri, para facilitar testes).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::Connection;
use serde_json::json;

use crate::db;
use crate::error::AppResult;
use crate::extensions::net::Net;
use crate::import::{EVENT_LIBRARY, Emit, Importer};
use crate::library;
use crate::pages::PageService;

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
}

impl Core {
    pub fn init(data_dir: PathBuf, emit: Emit) -> AppResult<Self> {
        let paths = AppPaths::new(data_dir)?;
        let conn = db::open(&paths.db)?;
        library::mark_interrupted_jobs(&conn)?;
        let db_path = paths.db.clone();
        let importer = Importer::spawn(move || db::open(&db_path), paths.covers.clone(), emit.clone())?;
        // Capas pendentes ou geradas com parâmetros antigos são refeitas em segundo plano.
        importer.enqueue_covers();
        Ok(Core { pages: PageService::new(paths.pages_cache.clone()), paths, db: Mutex::new(conn), importer, emit, guided_lock: Mutex::new(()), net: Arc::new(Net::from_env()) })
    }

    pub fn db(&self) -> MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Confere se os arquivos ainda existem e atualiza o indicador `missing`.
    /// Usa uma conexão própria para não segurar o banco principal.
    pub fn verify_files(&self) -> AppResult<(usize, usize)> {
        let conn = db::open(&self.paths.db)?;
        let infos = library::all_file_infos(&conn)?;
        let (mut missing, mut restored) = (0, 0);
        for info in infos {
            let exists = Path::new(&info.path).exists();
            if !exists && !info.missing {
                library::set_missing(&conn, info.id, true)?;
                missing += 1;
            } else if exists && info.missing {
                library::set_missing(&conn, info.id, false)?;
                restored += 1;
            }
        }
        if missing + restored > 0 {
            (self.emit)(EVENT_LIBRARY, json!({}));
        }
        Ok((missing, restored))
    }
}
