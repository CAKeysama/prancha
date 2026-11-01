//! Comandos expostos à interface. Operações de disco/banco rodam fora da thread principal.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;
use serde_json::{Value, json};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::core::Core;
use crate::covers::{self, COVER_PARAMS_VERSION};
use crate::error::{AppError, AppResult};
use crate::formats::{self, COMIC_EXTENSIONS, ComicFormat};
use crate::guided::{self, store::{self, GuidedPage, GuidedRegion}};
use crate::import::{self, EVENT_LIBRARY};
use crate::library::{
    self, CollectionInfo, ComicDetails, ComicSummary, HomeData, LibraryQuery, MetadataPatch, Progress,
};
use crate::pages::dir_size;

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

// ---------------------------------------------------------------------------
// App e configurações
// ---------------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    name: String,
    version: String,
    tauri_version: String,
    data_dir: String,
    schema_version: i64,
    supported_extensions: Vec<String>,
}

#[tauri::command]
pub async fn get_app_info(app: AppHandle, state: S<'_>) -> AppResult<AppInfo> {
    let info = app.package_info();
    Ok(AppInfo {
        name: info.name.clone(),
        version: info.version.to_string(),
        tauri_version: tauri::VERSION.to_string(),
        data_dir: state.paths.data_dir.to_string_lossy().to_string(),
        schema_version: crate::db::schema_version(),
        supported_extensions: COMIC_EXTENSIONS.iter().map(|s| s.to_string()).collect(),
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
    blocking(&state, move |c| {
        let mut db = c.db();
        library::update_metadata(&mut db, id, &patch)?;
        let d = library::details(&db, id)?;
        drop(db);
        library_changed(c);
        Ok(d)
    })
    .await
}

#[tauri::command]
pub async fn set_favorite(state: S<'_>, id: i64, favorite: bool) -> AppResult<()> {
    blocking(&state, move |c| {
        library::set_favorite(&c.db(), id, favorite)?;
        library_changed(c);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn set_read_status(state: S<'_>, id: i64, status: String) -> AppResult<()> {
    blocking(&state, move |c| {
        library::set_read_status(&c.db(), id, &status)?;
        library_changed(c);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn reset_progress(state: S<'_>, id: i64) -> AppResult<()> {
    blocking(&state, move |c| {
        library::reset_progress(&c.db(), id)?;
        library_changed(c);
        Ok(())
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoveResult {
    removed: bool,
    trashed: bool,
}

/// Remove do catálogo. Com `delete_original`, o arquivo vai para a Lixeira do sistema (reversível).
#[tauri::command]
pub async fn remove_comic(state: S<'_>, id: i64, delete_original: bool) -> AppResult<RemoveResult> {
    blocking(&state, move |c| {
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
        library_changed(c);
        Ok(RemoveResult { removed: true, trashed })
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
        let meta = std::fs::metadata(&path)?;
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let size = if format == ComicFormat::Folder { listing.pages.iter().map(|p| p.size as i64).sum() } else { meta.len() as i64 };
        {
            let db = c.db();
            library::relink(&db, id, &path_str, &name, size, None)?;
            if !matched {
                db.execute(
                    "UPDATE comics SET fingerprint = ?1, format = ?2, page_count = ?3 WHERE id = ?4",
                    rusqlite::params![fp, format.as_str(), formats::page_count(format, &listing) as i64, id],
                )?;
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
    blocking(&state, move |c| {
        let s = library::save_progress(&c.db(), id, page_index, page_count, scroll_ratio)?;
        library_changed(c);
        Ok(s)
    })
    .await
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
    blocking(&state, |c| library::pending_pdf_covers(&c.db())).await
}

/// Recebe a primeira página de um PDF renderizada pelo PDF.js. Os bytes são validados
/// e normalizados no backend antes de virarem miniatura.
#[tauri::command]
pub async fn save_pdf_cover(state: S<'_>, id: i64, image: Vec<u8>, page_count: Option<i64>) -> AppResult<()> {
    blocking(&state, move |c| {
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
        drop(db);
        library_changed(c);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn report_pdf_cover_failure(state: S<'_>, id: i64, message: String) -> AppResult<()> {
    blocking(&state, move |c| {
        let msg: String = message.chars().take(300).collect();
        library::set_cover(&c.db(), id, "placeholder", None, None, Some(&msg), COVER_PARAMS_VERSION)?;
        library_changed(c);
        Ok(())
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
    blocking(&state, move |c| {
        library::delete_collection(&c.db(), id)?;
        library_changed(c);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn set_comic_collections(state: S<'_>, comic_id: i64, collection_ids: Vec<i64>) -> AppResult<()> {
    blocking(&state, move |c| {
        library::set_comic_collections(&mut c.db(), comic_id, &collection_ids)?;
        library_changed(c);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn add_to_collection(state: S<'_>, collection_id: i64, comic_ids: Vec<i64>) -> AppResult<()> {
    blocking(&state, move |c| {
        library::add_to_collection(&mut c.db(), collection_id, &comic_ids)?;
        library_changed(c);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn remove_from_collection(state: S<'_>, collection_id: i64, comic_id: i64) -> AppResult<()> {
    blocking(&state, move |c| {
        library::remove_from_collection(&c.db(), collection_id, comic_id)?;
        library_changed(c);
        Ok(())
    })
    .await
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
    comic_count: i64,
    missing_count: i64,
}

#[tauri::command]
pub async fn storage_info(state: S<'_>) -> AppResult<StorageInfo> {
    blocking(&state, |c| {
        let (comic_count, missing_count) = library::counts(&c.db())?;
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
            comic_count,
            missing_count,
        })
    })
    .await
}

/// Apaga as miniaturas em cache e agenda a recriação. Progresso e HQs originais não são tocados.
#[tauri::command]
pub async fn clear_cover_cache(state: S<'_>) -> AppResult<u64> {
    blocking(&state, |c| {
        let freed = dir_size(&c.paths.covers);
        if let Ok(entries) = std::fs::read_dir(&c.paths.covers) {
            for e in entries.filter_map(Result::ok) {
                let _ = std::fs::remove_file(e.path());
            }
        }
        library::reset_all_covers(&c.db())?;
        c.importer.enqueue_covers();
        library_changed(c);
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
    blocking(&state, |c| {
        library::reset_all_covers(&c.db())?;
        c.importer.enqueue_covers();
        library_changed(c);
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
