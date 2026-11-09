//! Comandos expostos à interface. Operações de disco/banco rodam fora da thread principal.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use serde_json::{Value, json};
use tauri::ipc::Channel;
use tauri::utils::config::BundleType;
use tauri::webview::NewWindowResponse;
use tauri::{AppHandle, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::{Error as UpdaterError, Update, UpdaterExt};

use crate::core::Core;
use crate::covers::{self, COVER_PARAMS_VERSION};
use crate::error::{AppError, AppResult};
use crate::extensions::downloads::{self, DownloadView};
use crate::extensions::install::{self, InstalledExtension};
use crate::extensions::net::Cancel;
use crate::extensions::rating::State as RatingState;
use crate::extensions::rating_store::{self, RatingDetails};
use crate::extensions::recommended::{self, RecommendedList};
use crate::extensions::reading::{self, ChapterReadInfo};
use crate::extensions::repository::{self, CatalogItem, CatalogQuery, RepositoryInfo, RepositoryPreview};
use crate::extensions::sources::{self, BrowsePage, ChapterSession, ComicOrigin, SeriesCard, SeriesView, SourceSummary};
use crate::extensions::suwayomi;
use crate::formats::{self, COMIC_EXTENSIONS};
use crate::guided::{self, store::{self, GuidedPage, GuidedRegion}};
use crate::import::{self, EVENT_LIBRARY};
use crate::library::{
    self, CollectionInfo, ComicDetails, ComicSummary, HomeData, LibraryQuery, MetadataPatch, Progress,
};
use crate::notifications::{self, NotificationPage};
use crate::pages::dir_size;
use crate::updates;

type S<'a> = State<'a, Arc<Core>>;

async fn blocking<T: Send + 'static>(
    core: &Arc<Core>,
    f: impl FnOnce(&Core) -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    let core = core.clone();
    tauri::async_runtime::spawn_blocking(move || f(&core))
        .await
        .map_err(|e| AppError::user("internal", format!("Falha interna: {e}")))?
}

fn library_changed(core: &Core) {
    (core.emit)(EVENT_LIBRARY, json!({}));
}

/// Como `blocking`, e avisa a interface de que a biblioteca mudou quando `f` dá certo.
async fn changing<T: Send + 'static>(
    core: &Arc<Core>,
    f: impl FnOnce(&Core) -> AppResult<T> + Send + 'static,
) -> AppResult<T> {
    blocking(core, move |c| {
        let out = f(c)?;
        library_changed(c);
        Ok(out)
    })
    .await
}

// ---------------------------------------------------------------------------
// App e configurações
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: String,
    tauri_version: String,
    schema_version: i64,
}

#[tauri::command]
pub async fn get_app_info(app: AppHandle) -> AppResult<AppInfo> {
    Ok(AppInfo {
        version: app.package_info().version.to_string(),
        tauri_version: tauri::VERSION.to_string(),
        schema_version: crate::db::schema_version(),
    })
}

#[tauri::command]
pub async fn get_settings(state: S<'_>) -> AppResult<HashMap<String, Value>> {
    blocking(&state, |c| library::get_settings(&c.db())).await
}

#[tauri::command]
pub async fn set_setting(state: S<'_>, key: String, value: Value) -> AppResult<()> {
    blocking(&state, move |c| library::set_setting(&c.db(), &key, &value)).await
}

// ---------------------------------------------------------------------------
// Biblioteca
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_comics(state: S<'_>, query: LibraryQuery) -> AppResult<Vec<ComicSummary>> {
    blocking(&state, move |c| library::list_comics(&c.db(), &query)).await
}

#[tauri::command]
pub async fn get_home(state: S<'_>) -> AppResult<HomeData> {
    blocking(&state, |c| library::home(&c.db())).await
}

#[tauri::command]
pub async fn get_comic(state: S<'_>, id: i64) -> AppResult<ComicDetails> {
    blocking(&state, move |c| {
        // Confere a existência do arquivo a cada abertura dos detalhes.
        let info = library::file_info(&c.db(), id)?;
        let exists = Path::new(&info.path).exists();
        if exists == info.missing {
            library::set_missing(&c.db(), id, !exists)?;
        }
        library::details(&c.db(), id)
    })
    .await
}

#[tauri::command]
pub async fn list_tags(state: S<'_>) -> AppResult<Vec<String>> {
    blocking(&state, |c| library::all_tags(&c.db())).await
}

#[tauri::command]
pub async fn update_metadata(state: S<'_>, id: i64, patch: MetadataPatch) -> AppResult<ComicDetails> {
    changing(&state, move |c| {
        let mut db = c.db();
        library::update_metadata(&mut db, id, &patch)?;
        library::details(&db, id)
    })
    .await
}

#[tauri::command]
pub async fn set_favorite(state: S<'_>, id: i64, favorite: bool) -> AppResult<()> {
    changing(&state, move |c| library::set_favorite(&c.db(), id, favorite)).await
}

#[tauri::command]
pub async fn set_read_status(state: S<'_>, id: i64, status: String) -> AppResult<()> {
    changing(&state, move |c| library::set_read_status(&c.db(), id, &status)).await
}

#[tauri::command]
pub async fn reset_progress(state: S<'_>, id: i64) -> AppResult<()> {
    changing(&state, move |c| library::reset_progress(&c.db(), id)).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveResult {
    trashed: bool,
}

/// Remove do catálogo. Com `delete_original`, o arquivo vai para a Lixeira do sistema (reversível).
#[tauri::command]
pub async fn remove_comic(state: S<'_>, id: i64, delete_original: bool) -> AppResult<RemoveResult> {
    changing(&state, move |c| {
        let info = library::file_info(&c.db(), id)?;
        let mut trashed = false;
        if delete_original && Path::new(&info.path).exists() {
            trash::delete(&info.path).map_err(|e| {
                AppError::user(
                    "trash_failed",
                    format!("Não foi possível mover o arquivo original para a Lixeira ({e}). Nada foi removido."),
                )
            })?;
            trashed = true;
        }
        library::delete_comic(&c.db(), id)?;
        if let Some(file) = info.cover_file {
            let _ = std::fs::remove_file(c.paths.covers.join(file));
        }
        c.pages.forget_comic(id);
        Ok(RemoveResult { trashed })
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RelocateResult {
    /// `false` quando o usuário cancelou o seletor.
    picked: bool,
    /// O conteúdo novo é idêntico ao registrado.
    matched: bool,
    updated: bool,
    path: Option<String>,
}

/// Localiza novamente o arquivo de uma HQ ausente. Se o conteúdo for diferente,
/// só atualiza quando `force` for verdadeiro (a interface pede confirmação).
#[tauri::command]
pub async fn relocate_comic(app: AppHandle, state: S<'_>, id: i64, path: Option<String>, force: bool) -> AppResult<RelocateResult> {
    let core = state.inner().clone();
    let info = blocking(&core, move |c| library::file_info(&c.db(), id)).await?;
    let picked = match path {
        Some(p) => Some(PathBuf::from(p)),
        None => {
            let is_folder = info.format == "folder";
            pick_paths(app, if is_folder { PickMode::Folder } else { PickMode::Files }, "Localizar o arquivo da HQ")
                .await?
                .and_then(|v| v.into_iter().next())
        }
    };
    let Some(picked) = picked else {
        return Ok(RelocateResult { picked: false, matched: false, updated: false, path: None });
    };
    blocking(&core, move |c| {
        let path = import::canonical(&picked)?;
        let format = formats::detect(&path)?;
        let listing = formats::list(format, &path)?;
        let fp = import::fingerprint(format, &path, &listing)?;
        let matched = fp == info.fingerprint;
        if !matched && !force {
            return Ok(RelocateResult { picked: true, matched, updated: false, path: Some(path.to_string_lossy().into()) });
        }
        let path_str = path.to_string_lossy().to_string();
        if let Some(other) = library::find_by_path(&c.db(), &path_str)? {
            if other != id {
                return Err(AppError::user("duplicate", "Esse arquivo já está registrado como outra HQ da biblioteca."));
            }
        }
        let size = import::content_size(format, &listing, &std::fs::metadata(&path)?);
        {
            let db = c.db();
            library::relink(&db, id, &path_str, &import::display_name(&path), size)?;
            if !matched {
                library::set_content(&db, id, &fp, format.as_str(), import::known_page_count(format, &listing))?;
                import::apply_cover(&db, &c.paths.covers, id, format, &path, &listing)?;
            }
        }
        c.pages.forget_comic(id);
        library_changed(c);
        Ok(RelocateResult { picked: true, matched, updated: true, path: Some(path_str) })
    })
    .await
}

#[tauri::command]
pub async fn reveal_comic(app: AppHandle, state: S<'_>, id: i64) -> AppResult<()> {
    let info = blocking(&state, move |c| library::file_info(&c.db(), id)).await?;
    if !Path::new(&info.path).exists() {
        return Err(AppError::user("file_missing", "O arquivo não está mais nesse local."));
    }
    app.opener()
        .reveal_item_in_dir(&info.path)
        .map_err(|e| AppError::user("opener", format!("Não foi possível abrir a pasta ({e}).")))
}

#[tauri::command]
pub async fn open_data_dir(app: AppHandle, state: S<'_>) -> AppResult<()> {
    app.opener()
        .open_path(state.paths.data_dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| AppError::user("opener", format!("Não foi possível abrir a pasta ({e}).")))
}

// ---------------------------------------------------------------------------
// Importação
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PickMode {
    Files,
    Folder,
}

/// Em builds de depuração, `PRANCHA_E2E_PICK_FILE` aponta para um JSON
/// (`{"paths": [...]}` ou `{"cancel": true}`) consumido no lugar do diálogo nativo,
/// para que os testes E2E possam automatizar a escolha. A importação continua real.
#[cfg(debug_assertions)]
fn e2e_pick() -> Option<Option<Vec<PathBuf>>> {
    let file = std::env::var_os("PRANCHA_E2E_PICK_FILE")?;
    let text = std::fs::read_to_string(&file).ok()?;
    let _ = std::fs::remove_file(&file);
    let v: Value = serde_json::from_str(&text).ok()?;
    if v.get("cancel").and_then(Value::as_bool) == Some(true) {
        return Some(None);
    }
    let paths = v.get("paths")?.as_array()?.iter().filter_map(|p| p.as_str().map(PathBuf::from)).collect();
    Some(Some(paths))
}

#[cfg(not(debug_assertions))]
fn e2e_pick() -> Option<Option<Vec<PathBuf>>> {
    None
}

async fn pick_paths(app: AppHandle, mode: PickMode, title: &str) -> AppResult<Option<Vec<PathBuf>>> {
    if let Some(result) = e2e_pick() {
        return Ok(result);
    }
    let title = title.to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let dialog = app.dialog().file().set_title(&title);
        let picked = match mode {
            PickMode::Files => dialog
                .add_filter("Quadrinhos (CBZ, CBR, CB7, CBT, PDF)", COMIC_EXTENSIONS)
                .blocking_pick_files()
                .map(|v| v.into_iter().filter_map(|f| f.into_path().ok()).collect::<Vec<_>>()),
            PickMode::Folder => dialog.blocking_pick_folder().and_then(|f| f.into_path().ok()).map(|p| vec![p]),
        };
        Ok(picked)
    })
    .await
    .map_err(|e| AppError::user("internal", e.to_string()))?
}

#[tauri::command]
pub async fn pick_import_paths(app: AppHandle, mode: PickMode) -> AppResult<Option<Vec<String>>> {
    let title = match mode {
        PickMode::Files => "Adicionar quadrinhos",
        PickMode::Folder => "Adicionar uma pasta de quadrinhos",
    };
    Ok(pick_paths(app, mode, title)
        .await?
        .map(|v| v.into_iter().map(|p| p.to_string_lossy().to_string()).collect()))
}

#[tauri::command]
pub async fn start_import(state: S<'_>, paths: Vec<String>) -> AppResult<i64> {
    if paths.is_empty() {
        return Err(AppError::user("invalid", "Nenhum arquivo foi selecionado."));
    }
    blocking(&state, move |c| {
        let job_id = library::create_import_job(&c.db())?;
        c.importer.enqueue_import(job_id, paths.into_iter().map(PathBuf::from).collect());
        Ok(job_id)
    })
    .await
}

#[tauri::command]
pub async fn cancel_import(state: S<'_>, job_id: i64) -> AppResult<bool> {
    Ok(state.importer.cancel(job_id))
}

// ---------------------------------------------------------------------------
// Leitor
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReaderSession {
    comic: ComicSummary,
    path: String,
    page_count: i64,
    start: Progress,
}

#[tauri::command]
pub async fn open_comic(state: S<'_>, id: i64) -> AppResult<ReaderSession> {
    blocking(&state, move |c| {
        let info = library::file_info(&c.db(), id)?;
        if !Path::new(&info.path).exists() {
            library::set_missing(&c.db(), id, true)?;
            library_changed(c);
            return Err(AppError::user(
                "file_missing",
                format!(
                    "O arquivo desta HQ não foi encontrado em “{}”. Abra os detalhes e use “Localizar arquivo” para indicar o novo local.",
                    info.path
                ),
            ));
        }
        if info.missing {
            library::set_missing(&c.db(), id, false)?;
        }
        let listing = c.pages.listing(&info)?;
        let mut count = listing.page_count as i64;
        if count == 0 {
            // PDF muito grande: o PDF.js informará o total ao abrir.
            count = info.page_count.unwrap_or(0);
        }
        let db = c.db();
        if count > 0 && info.page_count != Some(count) {
            library::set_page_count(&db, id, count)?;
        }
        let start = library::mark_opened(&db, id, count.max(1))?;
        let comic = library::summary(&db, id)?;
        drop(db);
        library_changed(c);
        Ok(ReaderSession { comic, path: info.path, page_count: count, start })
    })
    .await
}

#[tauri::command]
pub async fn save_progress(state: S<'_>, id: i64, page_index: i64, page_count: i64, scroll_ratio: f64) -> AppResult<String> {
    changing(&state, move |c| library::save_progress(&c.db(), id, page_index, page_count, scroll_ratio)).await
}

/// O PDF.js informa o número real de páginas de um PDF.
#[tauri::command]
pub async fn set_pdf_page_count(state: S<'_>, id: i64, page_count: i64) -> AppResult<()> {
    if page_count <= 0 {
        return Err(AppError::user("invalid", "Número de páginas inválido."));
    }
    blocking(&state, move |c| {
        let info = library::file_info(&c.db(), id)?;
        if info.format != "pdf" {
            return Err(AppError::user("invalid", "Apenas PDFs aceitam essa atualização."));
        }
        if info.page_count != Some(page_count) {
            library::set_page_count(&c.db(), id, page_count)?;
            library_changed(c);
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn pending_pdf_covers(state: S<'_>) -> AppResult<Vec<i64>> {
    blocking(&state, |c| library::pending_covers(&c.db(), true)).await
}

/// Recebe a primeira página de um PDF renderizada pelo PDF.js. Os bytes são validados
/// e normalizados no backend antes de virarem miniatura.
#[tauri::command]
pub async fn save_pdf_cover(state: S<'_>, id: i64, image: Vec<u8>, page_count: Option<i64>) -> AppResult<()> {
    changing(&state, move |c| {
        let info = library::file_info(&c.db(), id)?;
        if info.format != "pdf" {
            return Err(AppError::user("invalid", "Apenas PDFs aceitam capa renderizada pela interface."));
        }
        let db = c.db();
        if let Some(n) = page_count.filter(|n| *n > 0) {
            if info.page_count != Some(n) {
                library::set_page_count(&db, id, n)?;
            }
        }
        match covers::normalize_rendered(&image) {
            Ok(jpeg) => {
                let file = import::write_cover_file(&c.paths.covers, id, &jpeg)?;
                library::set_cover(&db, id, "first_page", Some(&file), Some(0), None, COVER_PARAMS_VERSION)?;
            }
            Err(e) => library::set_cover(&db, id, "placeholder", None, None, Some(&e), COVER_PARAMS_VERSION)?,
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn report_pdf_cover_failure(state: S<'_>, id: i64, message: String) -> AppResult<()> {
    changing(&state, move |c| {
        let msg: String = message.chars().take(300).collect();
        library::set_cover(&c.db(), id, "placeholder", None, None, Some(&msg), COVER_PARAMS_VERSION)
    })
    .await
}

// ---------------------------------------------------------------------------
// Leitura Guiada
// ---------------------------------------------------------------------------

/// Analisa os bytes de uma página e grava o resultado. Uma análise por vez, para não repetir
/// o trabalho quando a pré-carga e a leitura pedem a mesma página.
fn analyze_page(c: &Core, info: &library::ComicFileInfo, page: i64, rtl: bool, image: &[u8]) -> AppResult<GuidedPage> {
    let decoded = covers::decode_limited(image)
        .map_err(|e| AppError::user("undecodable", format!("Não foi possível analisar esta página ({e}).")))?;
    let analysis = guided::analyze(&decoded);
    let mut db = c.db();
    store::save_analysis(&mut db, info.id, page, &info.fingerprint, &analysis)?;
    store::read_page(&db, info.id, page, &info.fingerprint, rtl)?
        .ok_or_else(|| AppError::user("internal", "A análise não foi gravada."))
}

/// Regiões da página: correções do usuário ou análise automática (feita agora, se faltar).
/// PDFs são desenhados pela interface: sem análise guardada, devolve `None` e ela envia a imagem.
#[tauri::command]
pub async fn guided_page(state: S<'_>, comic_id: i64, page_index: i64, rtl: bool) -> AppResult<Option<GuidedPage>> {
    blocking(&state, move |c| {
        let info = library::file_info(&c.db(), comic_id)?;
        let _one_at_a_time = c.guided_lock.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(page) = store::read_page(&c.db(), comic_id, page_index, &info.fingerprint, rtl)? {
            return Ok(Some(page));
        }
        if info.format == "pdf" {
            return Ok(None);
        }
        let (bytes, _) = c.pages.read_page(&info, page_index as usize)?;
        analyze_page(c, &info, page_index, rtl, &bytes).map(Some)
    })
    .await
}

/// Recebe uma página de PDF desenhada pelo PDF.js.
#[tauri::command]
pub async fn guided_analyze_image(state: S<'_>, comic_id: i64, page_index: i64, rtl: bool, image: Vec<u8>) -> AppResult<GuidedPage> {
    blocking(&state, move |c| {
        let info = library::file_info(&c.db(), comic_id)?;
        let _one_at_a_time = c.guided_lock.lock().unwrap_or_else(|e| e.into_inner());
        analyze_page(c, &info, page_index, rtl, &image)
    })
    .await
}

#[tauri::command]
pub async fn guided_save_corrections(state: S<'_>, comic_id: i64, page_index: i64, regions: Vec<GuidedRegion>) -> AppResult<()> {
    blocking(&state, move |c| {
        let info = library::file_info(&c.db(), comic_id)?;
        store::save_corrections(&mut c.db(), comic_id, page_index, &info.fingerprint, &regions)
    })
    .await
}

#[tauri::command]
pub async fn guided_clear_corrections(state: S<'_>, comic_id: i64, page_index: i64) -> AppResult<()> {
    blocking(&state, move |c| store::clear_corrections(&mut c.db(), comic_id, page_index)).await
}

// ---------------------------------------------------------------------------
// Coleções
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_collections(state: S<'_>) -> AppResult<Vec<CollectionInfo>> {
    blocking(&state, |c| library::list_collections(&c.db())).await
}

#[tauri::command]
pub async fn create_collection(state: S<'_>, name: String) -> AppResult<i64> {
    blocking(&state, move |c| library::create_collection(&c.db(), &name)).await
}

#[tauri::command]
pub async fn rename_collection(state: S<'_>, id: i64, name: String) -> AppResult<()> {
    blocking(&state, move |c| library::rename_collection(&c.db(), id, &name)).await
}

#[tauri::command]
pub async fn delete_collection(state: S<'_>, id: i64) -> AppResult<()> {
    changing(&state, move |c| library::delete_collection(&c.db(), id)).await
}

#[tauri::command]
pub async fn set_comic_collections(state: S<'_>, comic_id: i64, collection_ids: Vec<i64>) -> AppResult<()> {
    changing(&state, move |c| library::set_comic_collections(&mut c.db(), comic_id, &collection_ids)).await
}

#[tauri::command]
pub async fn add_to_collection(state: S<'_>, collection_id: i64, comic_ids: Vec<i64>) -> AppResult<()> {
    changing(&state, move |c| library::add_to_collection(&mut c.db(), collection_id, &comic_ids)).await
}

#[tauri::command]
pub async fn remove_from_collection(state: S<'_>, collection_id: i64, comic_id: i64) -> AppResult<()> {
    changing(&state, move |c| library::remove_from_collection(&c.db(), collection_id, comic_id)).await
}

// ---------------------------------------------------------------------------
// Armazenamento e manutenção
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageInfo {
    data_dir: String,
    database_bytes: u64,
    covers_bytes: u64,
    pages_bytes: u64,
}

#[tauri::command]
pub async fn storage_info(state: S<'_>) -> AppResult<StorageInfo> {
    blocking(&state, |c| {
        let db_bytes = ["prancha.db", "prancha.db-wal", "prancha.db-shm"]
            .iter()
            .filter_map(|f| std::fs::metadata(c.paths.data_dir.join(f)).ok())
            .map(|m| m.len())
            .sum();
        Ok(StorageInfo {
            data_dir: c.paths.data_dir.to_string_lossy().to_string(),
            database_bytes: db_bytes,
            covers_bytes: dir_size(&c.paths.covers),
            pages_bytes: c.pages.cache_size(),
        })
    })
    .await
}

/// Apaga as miniaturas em cache e agenda a recriação. Progresso e HQs originais não são tocados.
#[tauri::command]
pub async fn clear_cover_cache(state: S<'_>) -> AppResult<u64> {
    changing(&state, |c| {
        let freed = dir_size(&c.paths.covers);
        if let Ok(entries) = std::fs::read_dir(&c.paths.covers) {
            for e in entries.filter_map(Result::ok) {
                let _ = std::fs::remove_file(e.path());
            }
        }
        library::reset_all_covers(&c.db())?;
        c.importer.enqueue_covers();
        Ok(freed)
    })
    .await
}

#[tauri::command]
pub async fn clear_page_cache(state: S<'_>) -> AppResult<u64> {
    blocking(&state, |c| Ok(c.pages.clear_cache())).await
}

/// Refaz todas as capas (útil após trocar arquivos de lugar ou atualizar o app).
#[tauri::command]
pub async fn regenerate_covers(state: S<'_>) -> AppResult<()> {
    changing(&state, |c| {
        library::reset_all_covers(&c.db())?;
        c.importer.enqueue_covers();
        Ok(())
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyResult {
    missing: usize,
    restored: usize,
}

#[tauri::command]
pub async fn verify_library(state: S<'_>) -> AppResult<VerifyResult> {
    blocking(&state, |c| {
        let (missing, restored) = c.verify_files()?;
        Ok(VerifyResult { missing, restored })
    })
    .await
}

// ---------------------------------------------------------------------------
// Atualizações
// ---------------------------------------------------------------------------

type Updates<'a> = State<'a, updates::Session>;

/// A cada quanto o download avisa a interface.
const PROGRESS_STEP: u64 = 128 * 1024;
/// Tempo para o aviso aparecer: quando o instalador abre, o aplicativo fecha na hora.
const INSTALL_NOTICE: std::time::Duration = std::time::Duration::from_millis(800);

/// A origem oficial é a que veio configurada no próprio aplicativo.
fn release_endpoint(app: &AppHandle) -> String {
    let updater = app.config().plugins.0.get("updater");
    updater.and_then(|u| u["endpoints"][0].as_str()).unwrap_or_default().to_string()
}

async fn update_snapshot(app: &AppHandle, core: &Arc<Core>) -> AppResult<updates::Snapshot> {
    let current = app.package_info().version.clone();
    // Só o pacote MSI instala para todos os usuários e pede a confirmação do Windows.
    let needs_admin = matches!(tauri::utils::platform::bundle_type(), Some(BundleType::Msi));
    blocking(core, move |c| updates::snapshot(&c.db(), &current, needs_admin)).await
}

/// Procura a versão estável mais recente e descarta o que não vier das releases oficiais.
async fn find_update(app: &AppHandle, core: &Core) -> AppResult<Option<Update>> {
    let builder = app
        .updater_builder()
        .timeout(updates::CHECK_TIMEOUT)
        .version_comparator(|current, remote| updates::is_newer(&current, &remote.version));
    let result = match builder.build() {
        Ok(updater) => updater.check().await,
        Err(e) => Err(e),
    };
    let found = match result {
        Ok(found) => found,
        // A release não tem pacote para este sistema: não há o que oferecer.
        Err(UpdaterError::TargetNotFound(_) | UpdaterError::TargetsNotFound(_)) => None,
        Err(e) => {
            updates::log(&core.paths.data_dir, &format!("consulta: {e}"));
            return Err(updates::describe(&e));
        }
    };
    let endpoint = release_endpoint(app);
    Ok(found.filter(|u| {
        let official = updates::is_official_package(&endpoint, &u.version, u.download_url.as_str());
        if !official {
            updates::log(&core.paths.data_dir, &format!("pacote fora das releases oficiais: {}", u.download_url));
        }
        official
    }))
}

async fn record_check(app: &AppHandle, core: &Arc<Core>, found: Option<updates::ReleaseInfo>) -> AppResult<()> {
    let current = app.package_info().version.clone();
    blocking(core, move |c| {
        let version = found.as_ref().map(|r| r.version.clone());
        updates::record_check(&c.db(), &current, found)?;
        // Só uma versão encontrada de verdade vira aviso, e cada versão uma vez.
        if let Some(version) = version {
            notifications::publish(c, &updates::update_notice(notifications::Kind::UpdateAvailable, &version));
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn updates_snapshot(app: AppHandle, state: S<'_>) -> AppResult<updates::Snapshot> {
    update_snapshot(&app, &state).await
}

#[tauri::command]
pub async fn updates_check(app: AppHandle, state: S<'_>, session: Updates<'_>) -> AppResult<updates::Snapshot> {
    if updates::ENABLED {
        let update = find_update(&app, &state).await?;
        let found = update.as_ref().map(|u| updates::release_from_update(u, &release_endpoint(&app)));
        *session.update.lock().unwrap() = update;
        record_check(&app, &state, found).await?;
    }
    update_snapshot(&app, &state).await
}

#[tauri::command]
pub async fn updates_decline(state: S<'_>, version: String) -> AppResult<()> {
    blocking(&state, move |c| updates::decline(&c.db(), &version)).await
}

#[tauri::command]
pub async fn updates_notes_shown(state: S<'_>, version: String) -> AppResult<()> {
    blocking(&state, move |c| updates::notes_shown(&c.db(), &version)).await
}

/// Baixa, confere e abre o instalador. Se der certo, o instalador fecha o aplicativo e o abre de
/// novo; só a próxima execução confirma a atualização, em `updates::reconcile`.
async fn download_and_install(app: &AppHandle, core: &Arc<Core>, session: &updates::Session, on_progress: &Channel<updates::Progress>) -> AppResult<()> {
    let data_dir = core.paths.data_dir.clone();
    if core.importer.is_busy() {
        return Err(AppError::user("busy", "Aguarde a importação terminar para atualizar o Prancha."));
    }
    let known = session.update.lock().unwrap().clone();
    let mut update = match known {
        Some(update) => update,
        None => match find_update(app, core).await? {
            Some(update) => update,
            None => {
                record_check(app, core, None).await?;
                return Err(AppError::user("up_to_date", "O Prancha já está na versão mais recente."));
            }
        },
    };
    let release = updates::release_from_update(&update, &release_endpoint(app));
    let version = release.version.clone();
    blocking(core, move |c| updates::remember_release(&c.db(), &release)).await?;
    update.timeout = Some(updates::DOWNLOAD_TIMEOUT);

    // Tarefa própria para poder cancelar. O plugin confere a assinatura antes de devolver os bytes.
    let (task, chunks, finished) = (update.clone(), on_progress.clone(), on_progress.clone());
    let download = tauri::async_runtime::spawn(async move {
        let (mut downloaded, mut reported) = (0u64, 0u64);
        task.download(
            move |chunk, total| {
                downloaded += chunk as u64;
                if downloaded - reported >= PROGRESS_STEP || Some(downloaded) == total {
                    reported = downloaded;
                    let _ = chunks.send(updates::Progress::Downloading { downloaded, total });
                }
            },
            move || {
                let _ = finished.send(updates::Progress::Verifying);
            },
        )
        .await
    });
    let abort = download.inner().abort_handle();
    *session.abort_download.lock().unwrap() = Some(Box::new(move || abort.abort()));
    let outcome = download.await;
    *session.abort_download.lock().unwrap() = None;
    let bytes = match outcome {
        Ok(Ok(bytes)) => bytes,
        Ok(Err(e)) => {
            updates::log(&data_dir, &format!("download de {version}: {e}"));
            return Err(updates::describe(&e));
        }
        Err(_) => return Err(AppError::user("cancelled", "Download cancelado.")),
    };
    if let Some(expected) = updates::expected_sha256(&update.raw_json, update.download_url.as_str()) {
        if !updates::sha256_matches(&bytes, &expected) {
            updates::log(&data_dir, &format!("SHA-256 diferente do anunciado em {version}"));
            return Err(AppError::user("hash", "O arquivo baixado não confere com o publicado e foi descartado. Nada foi alterado."));
        }
    }

    let ready = version.clone();
    blocking(core, move |c| updates::mark_ready_to_install(&c.db(), &ready)).await?;
    let _ = on_progress.send(updates::Progress::Installing);
    let installed = tauri::async_runtime::spawn_blocking(move || {
        std::thread::sleep(INSTALL_NOTICE);
        update.install(bytes)
    })
    .await
    .map_err(|e| AppError::user("internal", format!("Falha interna: {e}")))?;
    if let Err(e) = installed {
        updates::log(&data_dir, &format!("instalador de {version}: {e}"));
        blocking(core, |c| updates::install_not_started(&c.db())).await?;
        return Err(updates::describe(&e));
    }
    Ok(())
}

#[tauri::command]
pub async fn updates_install(app: AppHandle, state: S<'_>, session: Updates<'_>, on_progress: Channel<updates::Progress>) -> AppResult<()> {
    if !updates::ENABLED {
        return Err(AppError::user("unavailable", "Atualizações não estão disponíveis nesta compilação."));
    }
    let outcome = download_and_install(&app, &state, &session, &on_progress).await;
    if let Err(error) = &outcome {
        let version = session.update.lock().unwrap().as_ref().map(|u| u.version.clone());
        if let Some(version) = version.filter(|_| !matches!(error.code(), "cancelled" | "up_to_date" | "busy")) {
            blocking(&state, move |c| Ok(notifications::publish(c, &updates::update_notice(notifications::Kind::UpdateFailed, &version)))).await?;
        }
    }
    outcome
}

#[tauri::command]
pub fn updates_cancel(session: Updates<'_>) {
    if let Some(abort) = session.abort_download.lock().unwrap().take() {
        abort();
    }
}

/// As notas vêm das releases do GitHub e ficam guardadas: uma consulta por hora, ou quando a pessoa pedir.
#[tauri::command]
pub async fn updates_history(app: AppHandle, state: S<'_>, force: bool) -> AppResult<updates::History> {
    let fresh = blocking(&state, |c| updates::history_is_fresh(&c.db())).await?;
    let url = updates::releases_api(&release_endpoint(&app));
    let stale = match url.filter(|_| updates::ENABLED && (force || !fresh)) {
        None => false,
        Some(url) => match updates::fetch_releases(&url).await {
            Ok(releases) => {
                let saved = releases.clone();
                blocking(&state, move |c| updates::store_history(&c.db(), &saved)).await?;
                return Ok(updates::History { releases, stale: false });
            }
            Err(e) => {
                updates::log(&state.paths.data_dir, &format!("histórico: {e}"));
                true
            }
        },
    };
    let releases = blocking(&state, |c| updates::load_releases(&c.db())).await?;
    Ok(updates::History { releases, stale })
}

#[tauri::command]
pub async fn updates_open_release(app: AppHandle, version: String) -> AppResult<()> {
    let invalid = || AppError::user("not_found", "Versão desconhecida.");
    semver::Version::parse(&version).map_err(|_| invalid())?;
    let url = updates::release_page(&release_endpoint(&app), &version).ok_or_else(invalid)?;
    open_in_browser(&app, &url)
}

/// Endereço fixo: a tela só manda o texto. O formulário é `.github/ISSUE_TEMPLATE/melhoria.yml`.
const IMPROVEMENT_FORM: &str = "https://github.com/CAKeysama/prancha/issues/new";
const IMPROVEMENT_TEMPLATE: &str = "melhoria.yml";
/// O campo `id: ideia` do formulário; o GitHub preenche campos pelo `id` na consulta do endereço.
const IMPROVEMENT_FIELD: &str = "ideia";
const IMPROVEMENT_TITLE_MAX: usize = 100;
const IMPROVEMENT_TEXT_MAX: usize = 1500;
/// Se a pessoa não estiver conectada, o GitHub repete o endereço inteiro no login; 4000 deixa folga para isso.
const IMPROVEMENT_URL_MAX: usize = 4000;

/// O texto vai codificado na consulta: `&` ou `#` digitados não viram outros parâmetros.
fn improvement_url(title: &str, text: &str) -> AppResult<String> {
    let (title, text) = (title.trim(), text.trim());
    let invalid = |message: &str| Err(AppError::user("invalid", message));
    if title.is_empty() || text.is_empty() {
        return invalid("Preencha o título e a descrição.");
    }
    if title.chars().count() > IMPROVEMENT_TITLE_MAX || text.chars().count() > IMPROVEMENT_TEXT_MAX {
        return invalid("O título ou a descrição passou do tamanho máximo.");
    }
    let url = url::Url::parse_with_params(IMPROVEMENT_FORM, [("template", IMPROVEMENT_TEMPLATE), ("title", &format!("Melhoria: {title}")), (IMPROVEMENT_FIELD, text)])
        .map_err(|e| AppError::user("internal", format!("Falha interna: {e}")))?;
    if url.as_str().len() > IMPROVEMENT_URL_MAX {
        return invalid("A sugestão ficou longa demais para enviar pelo navegador. Encurte a descrição e tente de novo.");
    }
    Ok(url.into())
}

/// Só abre o formulário já preenchido: quem envia é a pessoa, no GitHub.
#[tauri::command]
pub async fn open_improvement_form(app: AppHandle, title: String, text: String) -> AppResult<()> {
    open_in_browser(&app, &improvement_url(&title, &text)?)
}

fn open_in_browser(app: &AppHandle, url: &str) -> AppResult<()> {
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| AppError::user("opener", format!("Não foi possível abrir o navegador ({e}).")))
}

// ---------------------------------------------------------------------------
// Repositórios de extensões
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn repositories_list(state: S<'_>) -> AppResult<Vec<RepositoryInfo>> {
    blocking(&state, |c| repository::list(&c.db())).await
}

/// Lista curta mantida pelo projeto. Só descreve: adicionar continua pela revisão da chave.
#[tauri::command]
pub async fn repositories_recommended(state: S<'_>, refresh: bool) -> AppResult<RecommendedList> {
    blocking(&state, move |c| recommended::load(c, refresh)).await
}

#[tauri::command]
pub async fn repository_preview(state: S<'_>, address: String) -> AppResult<RepositoryPreview> {
    blocking(&state, move |c| repository::preview(c, &address)).await
}

#[tauri::command]
pub async fn repository_add(state: S<'_>, address: String, fingerprint: String) -> AppResult<RepositoryInfo> {
    blocking(&state, move |c| repository::add(c, &address, &fingerprint)).await
}

#[tauri::command]
pub async fn repository_accept_key(state: S<'_>, id: String, fingerprint: String) -> AppResult<RepositoryInfo> {
    blocking(&state, move |c| repository::accept_key(c, &id, &fingerprint)).await
}

#[tauri::command]
pub async fn repository_remove(state: S<'_>, id: String) -> AppResult<()> {
    blocking(&state, move |c| repository::remove(&c.db(), &id)).await
}

#[tauri::command]
pub async fn repository_set_enabled(state: S<'_>, id: String, enabled: bool) -> AppResult<RepositoryInfo> {
    blocking(&state, move |c| repository::set_enabled(&c.db(), &id, enabled)).await
}

#[tauri::command]
pub async fn repository_sync(state: S<'_>, id: String) -> AppResult<RepositoryInfo> {
    blocking(&state, move |c| repository::sync(c, &id)).await
}

#[tauri::command]
pub async fn repositories_sync_all(state: S<'_>) -> AppResult<Vec<RepositoryInfo>> {
    blocking(&state, |c| repository::sync_all(c)).await
}

// ---------------------------------------------------------------------------
// Extensões
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn extension_catalog(state: S<'_>, query: CatalogQuery) -> AppResult<Vec<CatalogItem>> {
    blocking(&state, move |c| repository::catalog(&c.db(), &query, env!("CARGO_PKG_VERSION"))).await
}

#[tauri::command]
pub async fn extension_catalog_languages(state: S<'_>) -> AppResult<Vec<String>> {
    blocking(&state, |c| repository::catalog_languages(&c.db())).await
}

#[tauri::command]
pub async fn extensions_list(state: S<'_>) -> AppResult<Vec<InstalledExtension>> {
    blocking(&state, |c| install::list(&c.db())).await
}

#[tauri::command]
pub async fn extension_install(state: S<'_>, repository_id: String, extension_id: String) -> AppResult<InstalledExtension> {
    blocking(&state, move |c| install::install(c, &repository_id, &extension_id)).await
}

#[tauri::command]
pub async fn extension_update(state: S<'_>, id: String) -> AppResult<InstalledExtension> {
    blocking(&state, move |c| install::update(c, &id)).await
}

#[tauri::command]
pub async fn extension_remove(state: S<'_>, id: String) -> AppResult<()> {
    blocking(&state, move |c| install::remove(c, &id)).await
}

#[tauri::command]
pub async fn extension_set_enabled(state: S<'_>, id: String, enabled: bool) -> AppResult<InstalledExtension> {
    blocking(&state, move |c| install::set_enabled(&c.db(), &id, enabled)).await
}

// ---------------------------------------------------------------------------
// Fontes e obras
// ---------------------------------------------------------------------------

/// Roda uma consulta que a interface pode abandonar com `source_cancel`.
async fn cancellable<T: Send + 'static>(state: &Arc<Core>, request_id: String, f: impl FnOnce(&Core, &Cancel) -> AppResult<T> + Send + 'static) -> AppResult<T> {
    blocking(state, move |c| {
        let cancel = c.requests.begin(&request_id);
        let out = f(c, &cancel);
        c.requests.end(&request_id);
        out
    })
    .await
}

#[tauri::command]
pub async fn sources_list(state: S<'_>) -> AppResult<Vec<SourceSummary>> {
    blocking(&state, |c| sources::available_sources(&c.db())).await
}

#[tauri::command]
pub async fn source_popular(state: S<'_>, request_id: String, extension_id: String, source_id: String, page: u32, kind: Option<String>) -> AppResult<BrowsePage> {
    cancellable(&state, request_id, move |c, cancel| sources::popular(c, &extension_id, &source_id, page, kind.as_deref(), Some(cancel))).await
}

#[tauri::command]
pub async fn source_latest(state: S<'_>, request_id: String, extension_id: String, source_id: String, page: u32, kind: Option<String>) -> AppResult<BrowsePage> {
    cancellable(&state, request_id, move |c, cancel| sources::latest(c, &extension_id, &source_id, page, kind.as_deref(), Some(cancel))).await
}

#[tauri::command]
pub async fn source_search(state: S<'_>, request_id: String, extension_id: String, source_id: String, query: String, page: u32, kind: Option<String>) -> AppResult<BrowsePage> {
    cancellable(&state, request_id, move |c, cancel| sources::search(c, &extension_id, &source_id, &query, page, kind.as_deref(), Some(cancel))).await
}

#[tauri::command]
pub async fn source_cancel(state: S<'_>, request_id: String) -> AppResult<()> {
    state.requests.cancel(&request_id);
    Ok(())
}

#[tauri::command]
pub async fn series_stored(state: S<'_>, id: i64) -> AppResult<SeriesView> {
    blocking(&state, move |c| sources::visible_series(&c.db(), id)).await
}

#[tauri::command]
pub async fn series_open(state: S<'_>, request_id: String, id: i64) -> AppResult<SeriesView> {
    cancellable(&state, request_id, move |c, cancel| sources::open_series(c, id, Some(cancel))).await
}

#[tauri::command]
pub async fn series_set_following(state: S<'_>, id: i64, following: bool) -> AppResult<SeriesCard> {
    changing(&state, move |c| sources::set_following(&c.db(), id, following)).await
}

const VERIFICATION_WINDOW: &str = "verificacao";
const MAIN_WINDOW: &str = "main";

/// Abre o site numa janela, e quem passa pela verificação é a pessoa, à mão. O Prancha não responde
/// por ninguém: só leva ao servidor das fontes o que o site entregou depois. A janela não tem acesso
/// a nada do app (as permissões valem só para `main`, e o protocolo recusa as outras), e é anônima
/// para cada verificação começar do zero.
fn verify_site(app: &AppHandle, core: &Core, address: &str) -> AppResult<bool> {
    let refused = |message: String| AppError::user("verification_failed", message);
    let url: tauri::Url = address.parse().map_err(|_| refused("O endereço deste site não é válido.".into()))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(refused("O endereço deste site não é válido.".into()));
    }
    let window = WebviewWindowBuilder::new(app, VERIFICATION_WINDOW, WebviewUrl::External(url.clone()))
        .title("Confirme no site e feche esta janela")
        .inner_size(560.0, 760.0)
        .user_agent(suwayomi::USER_AGENT)
        .incognito(true)
        .on_new_window(|_, _| NewWindowResponse::Deny)
        .on_download(|_, _| false)
        .build()
        .map_err(|e| refused(format!("Não foi possível abrir a janela: {e}")))?;

    let mut cookies = Vec::new();
    let mut cleared = false;
    while app.get_webview_window(VERIFICATION_WINDOW).is_some() {
        if app.get_webview_window(MAIN_WINDOW).is_none() {
            let _ = window.destroy();
            return Ok(false);
        }
        // A cada volta, porque depois de fechada a janela não responde mais.
        let pages = [Some(url.clone()), window.url().ok()];
        let found: Vec<_> = pages.into_iter().flatten().filter_map(|page| Some((page.host_str()?.to_string(), window.cookies_for_url(page).ok()?))).collect();
        if !found.is_empty() {
            cookies = found;
        }
        // A marca de que o site liberou. Sem ela, a janela fica até a pessoa fechar.
        cleared = cookies.iter().any(|(_, list)| list.iter().any(|cookie| cookie.name() == "cf_clearance"));
        if cleared {
            let _ = window.destroy();
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }

    let mut seen = std::collections::HashSet::new();
    let site: Vec<suwayomi::SiteCookie> = cookies
        .iter()
        .flat_map(|(host, list)| list.iter().map(move |cookie| (host, cookie)))
        .filter(|(host, cookie)| seen.insert((cookie.domain().unwrap_or(host.as_str()).to_string(), cookie.name().to_string())))
        .map(|(host, cookie)| suwayomi::SiteCookie { domain: cookie.domain().unwrap_or(host).trim_start_matches('.').to_string(), text: cookie.to_string() })
        .collect();
    if !site.is_empty() {
        suwayomi::hand_over_cookies(core, &site)?;
    }
    Ok(cleared)
}

/// Termina quando a janela do site fecha, para a tela que pediu tentar de novo, e diz se o site liberou.
#[tauri::command]
pub async fn source_verify(app: AppHandle, state: S<'_>, chapter_id: Option<i64>, series_id: Option<i64>, source_id: Option<String>) -> AppResult<bool> {
    if let Some(open) = app.get_webview_window(VERIFICATION_WINDOW) {
        let _ = open.set_focus();
        return Ok(false);
    }
    blocking(&state, move |core| {
        // Em builds de depuração, um site local faz o papel do site da fonte nos testes.
        let local = std::env::var("PRANCHA_E2E_VERIFY_URL").ok().filter(|_| cfg!(debug_assertions));
        let address = match local {
            Some(address) => address,
            None => sources::site_to_verify(core, chapter_id, series_id, source_id)?,
        };
        verify_site(&app, core, &address)
    })
    .await
}

#[tauri::command]
pub async fn series_library(state: S<'_>) -> AppResult<Vec<sources::LibrarySeries>> {
    blocking(&state, |c| sources::library_series(&c.db())).await
}

/// Por que a obra tem a classificação +18 que tem: estado, pontuação, evidências e correções.
#[tauri::command]
pub async fn series_rating(state: S<'_>, id: i64) -> AppResult<RatingDetails> {
    blocking(&state, move |c| rating_store::details(&c.db(), id)).await
}

/// Corrige a classificação +18 de uma obra. `rating` vazio volta ao automático.
#[tauri::command]
pub async fn series_set_rating(state: S<'_>, id: i64, rating: Option<String>, reason: Option<String>) -> AppResult<RatingDetails> {
    let rating = match rating.as_deref() {
        None => None,
        Some(text) => Some(RatingState::parse(text).ok_or_else(|| AppError::user("bad_request", "Classificação inválida."))?),
    };
    changing(&state, move |c| rating_store::set_manual(&mut c.db(), id, rating, reason.as_deref())).await
}

#[tauri::command]
pub async fn series_set_favorite(state: S<'_>, id: i64, favorite: bool) -> AppResult<()> {
    changing(&state, move |c| sources::set_favorite(&c.db(), id, favorite)).await
}

#[tauri::command]
pub async fn series_set_collections(state: S<'_>, id: i64, collection_ids: Vec<i64>) -> AppResult<()> {
    changing(&state, move |c| sources::set_series_collections(&mut c.db(), id, &collection_ids)).await
}

#[tauri::command]
pub async fn series_add_to_collection(state: S<'_>, collection_id: i64, ids: Vec<i64>) -> AppResult<()> {
    changing(&state, move |c| sources::add_series_to_collection(&mut c.db(), collection_id, &ids)).await
}

#[tauri::command]
pub async fn series_remove_from_collection(state: S<'_>, collection_id: i64, id: i64) -> AppResult<()> {
    changing(&state, move |c| sources::remove_series_from_collection(&c.db(), collection_id, id)).await
}

#[tauri::command]
pub async fn series_set_read(state: S<'_>, id: i64, read: bool) -> AppResult<()> {
    changing(&state, move |c| sources::set_series_read(&mut c.db(), id, read)).await
}

#[tauri::command]
pub async fn series_forget(state: S<'_>, id: i64) -> AppResult<()> {
    changing(&state, move |c| sources::forget_series(c, id)).await
}

#[tauri::command]
pub async fn series_set_chapter_sort(state: S<'_>, id: i64, sort: String) -> AppResult<SeriesCard> {
    blocking(&state, move |c| sources::set_chapter_sort(&c.db(), id, &sort)).await
}

#[tauri::command]
pub async fn series_set_reader_mode(state: S<'_>, id: i64, mode: String) -> AppResult<()> {
    blocking(&state, move |c| sources::set_reader_mode(&c.db(), id, &mode)).await
}

#[tauri::command]
pub async fn chapter_open(state: S<'_>, request_id: String, id: i64) -> AppResult<ChapterSession> {
    cancellable(&state, request_id, move |c, cancel| sources::open_chapter(c, id, Some(cancel))).await
}

#[tauri::command]
pub async fn chapter_save_progress(state: S<'_>, id: i64, page_index: i64, page_count: i64, scroll_ratio: f64) -> AppResult<()> {
    changing(&state, move |c| sources::save_chapter_progress(&c.db(), id, page_index, page_count, scroll_ratio)).await
}

#[tauri::command]
pub async fn chapter_reset_progress(state: S<'_>, id: i64) -> AppResult<()> {
    changing(&state, move |c| sources::reset_chapter_progress(&c.db(), id)).await
}

// Estado de leitura de cada capítulo. Ver extensions/reading.rs.

#[tauri::command]
pub async fn chapter_acknowledge(state: S<'_>, id: i64) -> AppResult<ChapterReadInfo> {
    changing(&state, move |c| reading::acknowledge(&c.db(), id)).await
}

#[tauri::command]
pub async fn chapter_reading_start(state: S<'_>, id: i64) -> AppResult<ChapterReadInfo> {
    changing(&state, move |c| reading::start(&mut c.db(), id)).await
}

/// Só mantém a sessão viva: não muda o que as telas mostram, e por isso não avisa a biblioteca.
#[tauri::command]
pub async fn chapter_reading_touch(state: S<'_>, id: i64) -> AppResult<bool> {
    blocking(&state, move |c| reading::touch(&c.db(), id)).await
}

#[tauri::command]
pub async fn chapter_reading_end(state: S<'_>, id: i64) -> AppResult<ChapterReadInfo> {
    changing(&state, move |c| reading::end(&c.db(), id)).await
}

#[tauri::command]
pub async fn chapter_reading_complete(state: S<'_>, id: i64) -> AppResult<ChapterReadInfo> {
    changing(&state, move |c| reading::complete(&mut c.db(), id)).await
}

#[tauri::command]
pub async fn chapters_set_read(state: S<'_>, ids: Vec<i64>, read: bool) -> AppResult<Vec<ChapterReadInfo>> {
    changing(&state, move |c| reading::set_read(&mut c.db(), &ids, read)).await
}

#[tauri::command]
pub async fn chapters_mark_previous_read(state: S<'_>, id: i64) -> AppResult<Vec<ChapterReadInfo>> {
    changing(&state, move |c| reading::mark_previous_read(&mut c.db(), id)).await
}

/// Consulta a fonte de cada obra acompanhada, uma de cada vez, e devolve a lista com os contadores de novos.
#[tauri::command]
pub async fn series_refresh_following(state: S<'_>, request_id: String) -> AppResult<Vec<SeriesCard>> {
    let out = cancellable(&state, request_id, move |c, cancel| sources::refresh_following(c, Some(cancel))).await?;
    library_changed(&state);
    Ok(out)
}

#[tauri::command]
pub async fn online_stop(state: S<'_>) -> AppResult<()> {
    blocking(&state, sources::stop_online).await
}

#[tauri::command]
pub async fn series_following(state: S<'_>) -> AppResult<Vec<SeriesCard>> {
    blocking(&state, |c| sources::list_following(&c.db())).await
}

#[tauri::command]
pub async fn comic_origin(state: S<'_>, id: i64) -> AppResult<Option<ComicOrigin>> {
    blocking(&state, move |c| sources::comic_origin(&c.db(), id)).await
}

// ---------------------------------------------------------------------------
// Downloads
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn downloads_list(state: S<'_>, limit: i64) -> AppResult<Vec<DownloadView>> {
    blocking(&state, move |c| downloads::list(&c.db(), limit)).await
}

#[tauri::command]
pub async fn downloads_enqueue(state: S<'_>, chapter_ids: Vec<i64>) -> AppResult<Vec<i64>> {
    let core = state.inner().clone();
    blocking(&state, move |_| downloads::enqueue(&core, &chapter_ids)).await
}

#[tauri::command]
pub async fn download_pause(state: S<'_>, id: i64) -> AppResult<()> {
    blocking(&state, move |c| downloads::pause(c, id)).await
}

#[tauri::command]
pub async fn download_resume(state: S<'_>, id: i64) -> AppResult<()> {
    let core = state.inner().clone();
    blocking(&state, move |_| downloads::resume(&core, id)).await
}

#[tauri::command]
pub async fn download_cancel(state: S<'_>, id: i64) -> AppResult<()> {
    blocking(&state, move |c| downloads::cancel(c, id)).await
}

#[tauri::command]
pub async fn download_retry(state: S<'_>, id: i64) -> AppResult<()> {
    let core = state.inner().clone();
    blocking(&state, move |_| downloads::retry(&core, id)).await
}

#[tauri::command]
pub async fn downloads_clear_finished(state: S<'_>) -> AppResult<usize> {
    blocking(&state, |c| downloads::clear_finished(&c.db())).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn improvement_form_points_to_a_template_in_the_repo() {
        let template = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../.github/ISSUE_TEMPLATE").join(IMPROVEMENT_TEMPLATE)).unwrap();
        assert!(template.lines().any(|line| line.trim() == format!("id: {IMPROVEMENT_FIELD}")), "o campo preenchido pelo endereço precisa existir no formulário");
    }

    #[test]
    fn improvement_url_carries_the_text_as_plain_values() {
        let url = url::Url::parse(&improvement_url("  Modo noturno & mais ", "Ler à noite #1\nsem cansar &labels=bug").unwrap()).unwrap();
        assert_eq!(url.as_str().split('?').next(), Some(IMPROVEMENT_FORM));
        let pairs: Vec<(String, String)> = url.query_pairs().map(|(k, v)| (k.into_owned(), v.into_owned())).collect();
        assert_eq!(
            pairs,
            [
                ("template".into(), IMPROVEMENT_TEMPLATE.into()),
                ("title".into(), "Melhoria: Modo noturno & mais".into()),
                (IMPROVEMENT_FIELD.into(), "Ler à noite #1\nsem cansar &labels=bug".into()),
            ]
        );
    }

    #[test]
    fn improvement_url_refuses_empty_or_oversized_text() {
        assert!(improvement_url("  ", "texto").is_err());
        assert!(improvement_url("Título", "\n ").is_err());
        assert!(improvement_url(&"a".repeat(IMPROVEMENT_TITLE_MAX + 1), "texto").is_err());
        assert!(improvement_url("Título", &"a".repeat(IMPROVEMENT_TEXT_MAX + 1)).is_err());
        // Dentro do limite de caracteres, mas com acentos que triplicam no endereço.
        assert!(improvement_url("Título", &"çã".repeat(IMPROVEMENT_TEXT_MAX / 2)).is_err());
        assert!(improvement_url("Título", &"a".repeat(IMPROVEMENT_TEXT_MAX)).is_ok());
    }
}

// ---------------------------------------------------------------------------
// Notificações
// ---------------------------------------------------------------------------

/// Toda alteração avisa a interface com a contagem já gravada: a badge não precisa consultar de novo.
async fn notifications_changing(core: &Arc<Core>, f: impl FnOnce(&Core) -> AppResult<i64> + Send + 'static) -> AppResult<i64> {
    blocking(core, move |c| {
        let unread = f(c)?;
        notifications::changed(c, unread);
        Ok(unread)
    })
    .await
}

#[tauri::command]
pub async fn notifications_list(state: S<'_>, query: notifications::ListQuery) -> AppResult<NotificationPage> {
    blocking(&state, move |c| notifications::list(&c.db(), &query)).await
}

#[tauri::command]
pub async fn notifications_unread(state: S<'_>) -> AppResult<i64> {
    blocking(&state, |c| notifications::unread_count(&c.db())).await
}

#[tauri::command]
pub async fn notifications_set_read(state: S<'_>, ids: Vec<i64>, read: bool) -> AppResult<i64> {
    notifications_changing(&state, move |c| notifications::set_read(&c.db(), &ids, read, crate::db::now_ms())).await
}

#[tauri::command]
pub async fn notifications_mark_all_read(state: S<'_>, category: Option<String>) -> AppResult<i64> {
    notifications_changing(&state, move |c| notifications::mark_all_read(&c.db(), category.as_deref(), crate::db::now_ms())).await
}

#[tauri::command]
pub async fn notifications_remove(state: S<'_>, ids: Vec<i64>) -> AppResult<i64> {
    notifications_changing(&state, move |c| notifications::remove(&c.db(), &ids)).await
}

#[tauri::command]
pub async fn notifications_clear_read(state: S<'_>) -> AppResult<i64> {
    notifications_changing(&state, |c| {
        let conn = c.db();
        notifications::clear_read(&conn)?;
        notifications::unread_count(&conn)
    })
    .await
}

#[tauri::command]
pub async fn notification_target(state: S<'_>, id: i64, action: Option<notifications::Action>) -> AppResult<notifications::Target> {
    blocking(&state, move |c| notifications::resolve(&c.db(), id, action)).await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeStatus {
    /// granted | denied | prompt
    permission: String,
}

#[tauri::command]
pub async fn notifications_native_status(app: AppHandle) -> AppResult<NativeStatus> {
    use tauri_plugin_notification::{NotificationExt, PermissionState};
    let permission = match app.notification().permission_state() {
        Ok(PermissionState::Granted) => "granted",
        Ok(PermissionState::Denied) => "denied",
        Ok(_) => "prompt",
        Err(_) => "denied",
    };
    Ok(NativeStatus { permission: permission.into() })
}

/// Pede a permissão (no desktop ela já vem concedida) e mostra um aviso de teste, sem guardar nada.
#[tauri::command]
pub async fn notifications_native_test(app: AppHandle) -> AppResult<NativeStatus> {
    use tauri_plugin_notification::{NotificationExt, PermissionState};
    let state = app.notification().request_permission().map_err(|e| AppError::user("native_failed", format!("O sistema não permitiu notificações: {e}")))?;
    if state != PermissionState::Granted {
        return Ok(NativeStatus { permission: "denied".into() });
    }
    app.notification()
        .builder()
        .title("Notificações do Prancha")
        .body("É assim que os avisos importantes aparecem fora da janela.")
        .show()
        .map_err(|e| AppError::user("native_failed", format!("O sistema não exibiu a notificação: {e}")))?;
    Ok(NativeStatus { permission: "granted".into() })
}
