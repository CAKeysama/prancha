//! Prancha — backend nativo.

pub mod commands;
pub mod core;
pub mod covers;
pub mod db;
pub mod error;
pub mod formats;
pub mod guided;
pub mod import;
pub mod library;
pub mod natural;
pub mod pages;
pub mod protocol;
pub mod text;

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
                    app.manage(core.clone());
                    std::thread::spawn(move || {
                        if let Err(e) = core.verify_files() {
                            log::warn!("verificação de arquivos falhou: {e}");
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
        ])
        .run(tauri::generate_context!())
        .expect("erro ao executar o Prancha");
}
