//! Arquivos no Android: o plugin Kotlin `FilesPlugin` (apps/android/src-tauri/gen/android) abre o seletor do
//! sistema, lê os endereços `content://` para o núcleo e avisa quando uma HQ é aberta por outro app.

use std::fs::File;
use std::io;
use std::os::fd::FromRawFd;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};
use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::plugin::mobile::PluginInvokeError;
use tauri::plugin::{Builder, PluginHandle, TauriPlugin};
use tauri::{Emitter, Manager, Wry};

use crate::core::Core;
use crate::import::{self, DuplicatePolicy};
use crate::location::{self, Entry, Meta, Provider};

/// Uma HQ aberta por outro app entrou na biblioteca: a interface pega o id com `take_opened_comic`.
pub const EVENT_OPENED: &str = "comic:opened";

/// O plugin Kotlin, guardado no estado do app para o seletor.
pub struct Files(PluginHandle<Wry>);

#[derive(Serialize)]
struct UriArgs<'a> {
    uri: &'a str,
}

#[derive(Serialize)]
struct WatchArgs {
    channel: Channel<serde_json::Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Stat {
    name: String,
    is_dir: bool,
    size: u64,
    modified: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Child {
    uri: String,
    name: String,
    is_dir: bool,
    size: u64,
}

#[derive(Deserialize)]
struct Children {
    items: Vec<Child>,
}

#[derive(Deserialize)]
struct Fd {
    fd: i32,
}

#[derive(Deserialize)]
struct Picked {
    uris: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct Opened {
    path: String,
}

fn io_error(e: PluginInvokeError) -> io::Error {
    match e {
        PluginInvokeError::InvokeRejected(r) if r.code.as_deref() == Some("not_found") => {
            io::Error::new(io::ErrorKind::NotFound, r.message.unwrap_or_default())
        }
        other => io::Error::other(other.to_string()),
    }
}

struct SafProvider(PluginHandle<Wry>);

impl Provider for SafProvider {
    fn open(&self, uri: &str) -> io::Result<File> {
        let Fd { fd } = self.0.run_mobile_plugin("open", UriArgs { uri }).map_err(io_error)?;
        // SAFETY: o Kotlin soltou o descritor do ParcelFileDescriptor (detachFd): ninguém mais o fecha.
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    fn stat(&self, uri: &str) -> io::Result<(String, Meta)> {
        let s: Stat = self.0.run_mobile_plugin("stat", UriArgs { uri }).map_err(io_error)?;
        let modified = s.modified.and_then(|ms| u64::try_from(ms).ok()).map(|ms| SystemTime::UNIX_EPOCH + Duration::from_millis(ms));
        Ok((s.name, Meta { is_dir: s.is_dir, is_file: !s.is_dir, len: s.size, modified }))
    }

    fn children(&self, uri: &str) -> io::Result<Vec<Entry>> {
        let c: Children = self.0.run_mobile_plugin("children", UriArgs { uri }).map_err(io_error)?;
        Ok(c.items
            .into_iter()
            .map(|i| Entry { path: PathBuf::from(i.uri), name: i.name, is_file: !i.is_dir, len: if i.is_dir { 0 } else { i.size } })
            .collect())
    }
}

/// Registra o plugin Kotlin e entrega ao núcleo a leitura dos endereços `content://`.
pub fn init() -> TauriPlugin<Wry> {
    Builder::new("prancha-files")
        .setup(|app, api| {
            let handle = api.register_android_plugin("app.prancha", "FilesPlugin")?;
            location::set_provider(Arc::new(SafProvider(handle.clone())));
            app.manage(Files(handle));
            Ok(())
        })
        .build()
}

/// O seletor do sistema: arquivos soltos ou uma pasta. `None` quando a pessoa cancela.
pub fn pick(app: &tauri::AppHandle, folder: bool) -> io::Result<Option<Vec<PathBuf>>> {
    let files = app.state::<Files>();
    let picked: Picked = files.0.run_mobile_plugin(if folder { "pickFolder" } else { "pickFiles" }, ()).map_err(io_error)?;
    Ok(picked.uris.filter(|u| !u.is_empty()).map(|u| u.into_iter().map(PathBuf::from).collect()))
}

/// Depois que o núcleo abriu: cada HQ aberta por outro app entra na biblioteca (ou é achada nela) e a
/// interface recebe o id para abrir o leitor.
pub fn watch_opened(app: &tauri::AppHandle, core: Arc<Core>) {
    let emitter = app.clone();
    let channel = Channel::new(move |body: InvokeResponseBody| {
        let opened: Opened = body.deserialize()?;
        let core = core.clone();
        let emitter = emitter.clone();
        std::thread::spawn(move || {
            let path = PathBuf::from(&opened.path);
            let result = import::process_one(&core.db(), &core.paths.covers, &path, DuplicatePolicy::Skip);
            match result.comic_id {
                Some(id) => {
                    // Guardado antes do aviso: na abertura do app pelo arquivo, a interface ainda não está ouvindo.
                    *emitter.state::<crate::commands::OpenedComic>().0.lock().unwrap() = Some(id);
                    let _ = emitter.emit(import::EVENT_LIBRARY, serde_json::json!({}));
                    let _ = emitter.emit(EVENT_OPENED, serde_json::json!({}));
                }
                None => log::warn!("HQ aberta por outro app não entrou na biblioteca: {:?}", result.message),
            }
        });
        Ok(())
    });
    // A chamada espera o Kotlin responder: não pode sair da thread principal.
    let handle = app.state::<Files>().0.clone();
    std::thread::spawn(move || {
        if let Err(e) = handle.run_mobile_plugin::<serde_json::Value>("watchOpened", WatchArgs { channel }) {
            log::warn!("abrir HQs de outros apps: {e}");
        }
    });
}
