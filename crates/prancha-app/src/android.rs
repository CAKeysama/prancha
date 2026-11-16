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

/// O plugin Kotlin da tela (`ScreenPlugin`): barras do sistema, modo imersivo e o Voltar na tela inicial.
pub struct Screen(PluginHandle<Wry>);

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

/// Registra o plugin Kotlin da tela. Cada plugin do Tauri carrega uma classe Kotlin só, por isso é outro.
pub fn init_screen() -> TauriPlugin<Wry> {
    Builder::new("prancha-screen")
        .setup(|app, api| {
            app.manage(Screen(api.register_android_plugin("app.prancha", "ScreenPlugin")?));
            Ok(())
        })
        .build()
}

/// O cofre do Android (`VaultPlugin`): os tokens do MyAnimeList cifrados por uma chave do Keystore.
struct KeystoreVault(PluginHandle<Wry>);

#[derive(Serialize, Deserialize)]
struct VaultData {
    data: String,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    (0..text.len()).step_by(2).map(|i| text.get(i..i + 2).and_then(|pair| u8::from_str_radix(pair, 16).ok())).collect()
}

impl KeystoreVault {
    fn call(&self, command: &str, bytes: &[u8]) -> prancha_core::error::AppResult<Vec<u8>> {
        let vault_error = |e: String| prancha_core::error::AppError::user("mal_vault", format!("Não foi possível usar o cofre do Android: {e}"));
        let out: VaultData = self.0.run_mobile_plugin(command, VaultData { data: hex(bytes) }).map_err(|e| vault_error(e.to_string()))?;
        unhex(&out.data).ok_or_else(|| vault_error("resposta inválida".into()))
    }
}

impl prancha_core::mal::vault::Vault for KeystoreVault {
    fn seal(&self, plain: &[u8]) -> prancha_core::error::AppResult<Vec<u8>> {
        self.call("seal", plain)
    }
    fn open(&self, sealed: &[u8]) -> prancha_core::error::AppResult<Vec<u8>> {
        self.call("unseal", sealed)
    }
}

pub fn init_vault() -> TauriPlugin<Wry> {
    Builder::new("prancha-vault")
        .setup(|_app, api| {
            prancha_core::mal::vault::register(Arc::new(KeystoreVault(api.register_android_plugin("app.prancha", "VaultPlugin")?)));
            Ok(())
        })
        .build()
}

/// O plugin Kotlin das extensões do Mihon (`MihonPlugin`), que as carrega dentro do app. O núcleo fala com ele por
/// `extensions::mihon::Host`, e cada chamada bloqueia a thread de quem pediu até a extensão responder.
struct MihonHost(PluginHandle<Wry>);

impl crate::extensions::mihon::Host for MihonHost {
    fn call(&self, command: &str, args: serde_json::Value) -> Result<serde_json::Value, String> {
        self.0.run_mobile_plugin(command, args).map_err(|e| match e {
            PluginInvokeError::InvokeRejected(rejected) => rejected.message.unwrap_or_else(|| "a extensão falhou sem explicar".into()),
            other => other.to_string(),
        })
    }
}

pub fn init_mihon() -> TauriPlugin<Wry> {
    Builder::new("prancha-mihon")
        .setup(|_app, api| {
            crate::extensions::mihon::register(Arc::new(MihonHost(api.register_android_plugin("app.prancha", "MihonPlugin")?)));
            Ok(())
        })
        .build()
}

/// O plugin Kotlin do que roda com o app fora da tela (`BackgroundPlugin`): o serviço de downloads.
pub struct Background(PluginHandle<Wry>);

pub fn init_background() -> TauriPlugin<Wry> {
    Builder::new("prancha-background")
        .setup(|app, api| {
            app.manage(Background(api.register_android_plugin("app.prancha", "BackgroundPlugin")?));
            Ok(())
        })
        .build()
}

/// O plugin Kotlin da atualização pelo APK (`UpdatePlugin`): onde o APK baixado fica e o instalador do sistema.
pub struct Updater(PluginHandle<Wry>);

pub fn init_updater() -> TauriPlugin<Wry> {
    Builder::new("prancha-updater")
        .setup(|app, api| {
            app.manage(Updater(api.register_android_plugin("app.prancha", "UpdatePlugin")?));
            Ok(())
        })
        .build()
}

#[derive(Deserialize)]
struct UpdatePath {
    path: PathBuf,
}

/// O arquivo do APK, na pasta de cache do app (o Android pode limpá-la, e o FileProvider do app a entrega ao instalador).
pub fn update_file(app: &tauri::AppHandle) -> io::Result<PathBuf> {
    app.state::<Updater>().0.run_mobile_plugin::<UpdatePath>("updateFile", ()).map(|r| r.path).map_err(io_error)
}

/// Abre o instalador do sistema para o APK de [`update_file`], já conferido.
pub fn install_apk(app: &tauri::AppHandle) -> io::Result<()> {
    app.state::<Updater>().0.run_mobile_plugin::<serde_json::Value>("installApk", ()).map(|_| ()).map_err(io_error)
}

/// O núcleo do app aberto neste processo. A verificação em segundo plano usa ele quando existe, para não abrir
/// o banco duas vezes nem repetir os avisos que ele mesmo já mostra.
static RUNNING: std::sync::OnceLock<Arc<Core>> = std::sync::OnceLock::new();

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ScheduleArgs<'a> {
    data_dir: &'a str,
}

/// Depois que o app abriu: segue a fila de downloads e agenda a verificação das obras acompanhadas.
pub fn app_started(app: &tauri::AppHandle, core: Arc<Core>) {
    follow_downloads(app, &core);
    let data_dir = core.paths.data_dir.to_string_lossy().into_owned();
    let _ = RUNNING.set(core);
    let app = app.clone();
    std::thread::spawn(move || {
        if let Err(e) = app.state::<Background>().0.run_mobile_plugin::<serde_json::Value>("scheduleFollowCheck", ScheduleArgs { data_dir: &data_dir }) {
            log::warn!("verificação em segundo plano não agendada: {e}");
        }
    });
}

/// Verifica as obras acompanhadas com o app fechado (o WorkManager chama, pelo `FollowWorker`). Devolve, em JSON,
/// os avisos do sistema que o app mostraria, para o Kotlin mostrar: sem o app aberto, ninguém mais mostra.
pub fn check_following(env: &mut jni::Env, context: jni::objects::JObject, data_dir: PathBuf) -> Result<String, String> {
    use prancha_core::notifications::{NativeNotice, NativeOutcome};
    use std::sync::Mutex;
    if let Some(core) = RUNNING.get() {
        prancha_core::extensions::sources::refresh_following(core, None).map_err(|e| e.to_string())?;
        return Ok("[]".into());
    }
    // As conexões HTTPS conferem certificados pela JVM, como na abertura do app.
    rustls_platform_verifier::android::init_with_env(env, context).map_err(|e| e.to_string())?;
    let core = Core::init(data_dir, Arc::new(|_: &str, _| {})).map_err(|e| e.to_string())?;
    let online = prancha_core::library::get_setting(&core.db(), "plugins.online").ok().flatten() == Some(serde_json::Value::Bool(true));
    if !online {
        return Ok("[]".into());
    }
    let notices: Arc<Mutex<Vec<serde_json::Value>>> = Arc::default();
    let collected = notices.clone();
    let _ = core.notifier.set(Arc::new(move |notice: &NativeNotice| {
        collected.lock().unwrap_or_else(|e| e.into_inner()).push(serde_json::json!({ "id": notice.id, "title": notice.title, "body": notice.body }));
        NativeOutcome::Shown
    }));
    prancha_core::extensions::sources::refresh_following(&core, None).map_err(|e| e.to_string())?;
    let list = std::mem::take(&mut *notices.lock().unwrap_or_else(|e| e.into_inner()));
    Ok(serde_json::Value::Array(list).to_string())
}

/// Liga o serviço em primeiro plano enquanto a fila de downloads tem capítulos e desliga quando ela esvazia.
/// O aviso da fila chega com ela travada: a chamada ao Kotlin vai para outra thread, e quem chega por último manda.
fn follow_downloads(app: &tauri::AppHandle, core: &Core) {
    use std::sync::atomic::{AtomicBool, Ordering};
    let wanted = Arc::new(AtomicBool::new(false));
    let app = app.clone();
    let _ = core.downloads.on_busy.set(Arc::new(move |busy| {
        wanted.store(busy, Ordering::SeqCst);
        let (app, wanted) = (app.clone(), wanted.clone());
        std::thread::spawn(move || {
            let on = wanted.load(Ordering::SeqCst);
            if let Err(e) = app.state::<Background>().0.run_mobile_plugin::<serde_json::Value>("setDownloading", SwitchArgs { on }) {
                log::warn!("serviço de downloads: {e}");
            }
        });
    }));
}

#[derive(Serialize)]
struct SwitchArgs {
    on: bool,
}

#[derive(Serialize)]
struct BarsArgs<'a> {
    dark: bool,
    color: &'a str,
}

/// Chamadas da interface para a tela. Esperam o Kotlin responder: rodam fora da thread principal.
pub fn set_immersive(app: &tauri::AppHandle, on: bool) -> io::Result<()> {
    app.state::<Screen>().0.run_mobile_plugin::<serde_json::Value>("setImmersive", SwitchArgs { on }).map(|_| ()).map_err(io_error)
}

pub fn set_bars(app: &tauri::AppHandle, dark: bool, color: &str) -> io::Result<()> {
    app.state::<Screen>().0.run_mobile_plugin::<serde_json::Value>("setBars", BarsArgs { dark, color }).map(|_| ()).map_err(io_error)
}

#[derive(Deserialize)]
struct Visible {
    visible: bool,
}

/// Se o app está na tela agora. Na dúvida, não: um aviso do sistema a mais é melhor que um perdido.
pub fn app_visible(app: &tauri::AppHandle) -> bool {
    app.state::<Screen>().0.run_mobile_plugin::<Visible>("isVisible", ()).is_ok_and(|v| v.visible)
}

/// No Android 13 ou mais novo, avisos do sistema pedem permissão. Ela é pedida no primeiro aviso, com o app na
/// tela (a caixa do sistema só aparece por cima dele), e uma vez por abertura: quem recusou continua com a central.
pub fn ask_notifications_once(app: &tauri::AppHandle, visible: bool) {
    use std::sync::atomic::{AtomicBool, Ordering};
    use tauri::plugin::PermissionState;
    use tauri_plugin_notification::NotificationExt;
    static ASKED: AtomicBool = AtomicBool::new(false);
    if !visible || !matches!(app.notification().permission_state(), Ok(PermissionState::Prompt | PermissionState::PromptWithRationale)) {
        return;
    }
    if ASKED.swap(true, Ordering::Relaxed) {
        return;
    }
    let app = app.clone();
    // A resposta espera a pessoa: fora da thread de quem publicou o aviso.
    std::thread::spawn(move || {
        if let Err(e) = app.notification().request_permission() {
            log::warn!("permissão de notificações: {e}");
        }
    });
}

pub fn move_to_back(app: &tauri::AppHandle) -> io::Result<()> {
    app.state::<Screen>().0.run_mobile_plugin::<serde_json::Value>("moveToBack", ()).map(|_| ()).map_err(io_error)
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
