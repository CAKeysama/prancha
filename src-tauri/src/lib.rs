//! Prancha — backend nativo.

pub mod commands;
pub mod core;
pub mod covers;
pub mod db;
pub mod error;
pub mod extensions;
pub mod formats;
pub mod guided;
pub mod import;
pub mod library;
pub mod natural;
pub mod pages;
pub mod protocol;
pub mod text;
pub mod updates;

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{Emitter, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

use crate::core::Core;
use crate::import::Emit;

/// Pasta de dados: `PRANCHA_DATA_DIR` (testes e instalações portáteis) ou a pasta padrão do sistema.
fn resolve_data_dir(app: &tauri::App) -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("PRANCHA_DATA_DIR").filter(|d| !d.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    app.path().app_data_dir().map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updates::Session::default())
        .register_asynchronous_uri_scheme_protocol("prancha", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            std::thread::spawn(move || {
                let core = app.state::<Arc<Core>>();
                responder.respond(protocol::handle(&core, &request));
            });
        })
        .setup(|app| {
            let data_dir = resolve_data_dir(app)?;
            let handle = app.handle().clone();
            let emit: Emit = Arc::new(move |event: &str, payload| {
                let _ = handle.emit(event, payload);
            });
            match Core::init(data_dir.clone(), emit) {
                Ok(core) => {
                    let core = Arc::new(core);
                    if let Err(e) = updates::reconcile(&core.db(), &app.package_info().version) {
                        updates::log(&core.paths.data_dir, &format!("conciliação: {e}"));
                    }
                    app.manage(core.clone());
                    std::thread::spawn(move || {
                        // Antes da verificação, que pode demorar: com a tela aberta, um download novo já estaria correndo.
                        if let Err(e) = extensions::downloads::on_startup(&core) {
                            log::warn!("downloads não retomados: {e}");
                        }
                        if let Err(e) = core.verify_files() {
                            log::warn!("verificação de arquivos falhou: {e}");
                        }
                        if let Err(e) = extensions::sources::prune_stale(&core, &core.db()) {
                            log::warn!("limpeza de obras antigas falhou: {e}");
                        }
                    });
                    Ok(())
                }
                Err(e) => {
                    app.dialog()
                        .message(format!(
                            "Não foi possível abrir os dados do Prancha em “{}”.\n\n{e}",
                            data_dir.display()
                        ))
                        .title("Prancha não pôde iniciar")
                        .kind(MessageDialogKind::Error)
                        .blocking_show();
                    Err(Box::new(e))
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_info,
            commands::get_settings,
            commands::set_setting,
            commands::list_comics,
            commands::get_home,
            commands::get_comic,
            commands::list_tags,
            commands::update_metadata,
            commands::set_favorite,
            commands::set_read_status,
            commands::reset_progress,
            commands::remove_comic,
            commands::relocate_comic,
            commands::reveal_comic,
            commands::open_data_dir,
            commands::pick_import_paths,
            commands::start_import,
            commands::cancel_import,
            commands::open_comic,
            commands::save_progress,
            commands::set_pdf_page_count,
            commands::pending_pdf_covers,
            commands::save_pdf_cover,
            commands::report_pdf_cover_failure,
            commands::guided_page,
            commands::guided_analyze_image,
            commands::guided_save_corrections,
            commands::guided_clear_corrections,
            commands::list_collections,
            commands::create_collection,
            commands::rename_collection,
            commands::delete_collection,
            commands::set_comic_collections,
            commands::add_to_collection,
            commands::remove_from_collection,
            commands::storage_info,
            commands::clear_cover_cache,
            commands::clear_page_cache,
            commands::regenerate_covers,
            commands::verify_library,
            commands::updates_snapshot,
            commands::updates_check,
            commands::updates_decline,
            commands::updates_notes_shown,
            commands::updates_install,
            commands::updates_cancel,
            commands::updates_history,
            commands::updates_open_release,
            commands::repositories_list,
            commands::repository_preview,
            commands::repository_add,
            commands::repository_accept_key,
            commands::repository_remove,
            commands::repository_set_enabled,
            commands::repository_sync,
            commands::repositories_sync_all,
            commands::extension_catalog,
            commands::extension_catalog_languages,
            commands::extensions_list,
            commands::extension_install,
            commands::extension_update,
            commands::extension_remove,
            commands::extension_set_enabled,
            commands::sources_list,
            commands::source_popular,
            commands::source_latest,
            commands::source_search,
            commands::source_cancel,
            commands::series_stored,
            commands::series_open,
            commands::series_set_following,
            commands::series_set_chapter_sort,
            commands::series_following,
            commands::comic_origin,
            commands::downloads_list,
            commands::downloads_enqueue,
            commands::download_pause,
            commands::download_resume,
            commands::download_cancel,
            commands::download_retry,
            commands::downloads_clear_finished,
        ])
        .run(tauri::generate_context!())
        .expect("erro ao executar o Prancha");
}
