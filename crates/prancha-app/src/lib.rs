//! Prancha — comandos da interface, protocolo `prancha://` e atualizador, sobre o núcleo `prancha-core`.
//! O aplicativo (apps/desktop) só monta o contexto do Tauri e chama [`run`].

#[cfg(target_os = "android")]
pub mod android;
pub mod commands;
pub mod protocol;
pub mod updates;

// Os módulos do núcleo ficam acessíveis como `crate::db`, `crate::core`… como eram antes da separação.
use prancha_core::{core, covers, db, error, extensions, formats, guided, import, library, location, mal, notifications, pages, telemetry};

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{Emitter, Manager, RunEvent};
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

/// Notificações do sistema: só com a janela fora de foco (com ela à vista, a central basta), e um
/// clique traz o Prancha para a frente e executa a ação da notificação.
fn connect_native_notifications(app: &tauri::AppHandle, core: &Core) {
    use notifications::{NativeNotice, NativeOutcome};
    use tauri_plugin_notification::{NotificationExt, PermissionState};
    let handle = app.clone();
    let notifier: notifications::Notifier = Arc::new(move |notice: &NativeNotice| {
        let focused = handle.get_webview_window("main").and_then(|w| w.is_focused().ok()).unwrap_or(false);
        if focused {
            return NativeOutcome::Skipped;
        }
        match handle.notification().permission_state() {
            Ok(PermissionState::Granted) => {}
            Ok(_) => return NativeOutcome::Denied,
            Err(_) => return NativeOutcome::Failed,
        }
        let Ok(id) = i32::try_from(notice.id) else { return NativeOutcome::Failed };
        match handle.notification().builder().id(id).title(&notice.title).body(&notice.body).show() {
            Ok(()) => NativeOutcome::Shown,
            Err(e) => {
                log::warn!("notificação do sistema: {e}");
                NativeOutcome::Failed
            }
        }
    });
    let _ = core.notifier.set(notifier);
    let handle = app.clone();
    let _ = app.notification().on_action(move |performed| {
        if performed.action_id() != "tap" {
            return;
        }
        if let Some(window) = handle.get_webview_window("main") {
            #[cfg(desktop)]
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
        if let Some(notification) = performed.notification() {
            let _ = handle.emit(notifications::EVENT_ACTIVATE, serde_json::json!({ "id": notification.id() }));
        }
    });
}

/// Conta a abertura (só com consentimento) e inicia a thread de envio, que dorme até ter o que mandar.
fn start_telemetry(app: &tauri::App, core: &Arc<Core>) {
    use tauri::utils::config::BundleType;
    let channel = if cfg!(debug_assertions) {
        "dev"
    } else {
        match tauri::utils::platform::bundle_type() {
            Some(BundleType::Nsis) => "nsis",
            Some(BundleType::Msi) => "msi",
            _ => "unknown",
        }
    };
    let meta = telemetry::AppMeta::current(app.package_info().version.to_string(), channel);
    if let Err(e) = core.telemetry.on_launch(&core.db(), meta) {
        log::warn!("telemetria na abertura: {e}");
    }
    core.telemetry.spawn();
}

/// No Android o reqwest confere certificados pela JVM: o verificador precisa do contexto do app antes da
/// primeira conexão HTTPS.
#[cfg(target_os = "android")]
fn init_android_tls() {
    let ctx = ndk_context::android_context();
    // SAFETY: os ponteiros da JVM e da Activity vêm do tao e valem enquanto o processo existir.
    let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) };
    let result = vm.attach_current_thread(|env| -> Result<(), jni::errors::Error> {
        let context = unsafe { jni::objects::JObject::from_raw(env, ctx.context().cast()) };
        rustls_platform_verifier::android::init_with_env(env, context)
    });
    if let Err(e) = result {
        log::warn!("verificador de certificados do Android: {e}");
    }
}

/// Monta e roda o aplicativo com o contexto (tauri.conf.json, ícones e frontend) de quem chama.
pub fn run(context: tauri::Context<tauri::Wry>) {
    let builder = tauri::Builder::default();
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_updater::Builder::new().build());
    #[cfg(target_os = "android")]
    let builder = builder.plugin(android::init());
    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .manage(updates::Session::default())
        .manage(commands::OpenedComic::default())
        .register_asynchronous_uri_scheme_protocol("prancha", |ctx, request, responder| {
            // A janela de verificação mostra um site de terceiros, que não pode ler nada da biblioteca.
            if ctx.webview_label() != "main" {
                responder.respond(protocol::forbidden());
                return;
            }
            let app = ctx.app_handle().clone();
            std::thread::spawn(move || {
                let core = app.state::<Arc<Core>>();
                responder.respond(protocol::handle(&core, &request));
            });
        })
        .setup(|app| {
            #[cfg(target_os = "android")]
            init_android_tls();
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
                    connect_native_notifications(app.handle(), &core);
                    start_telemetry(app, &core);
                    core.mal.spawn();
                    app.manage(core.clone());
                    #[cfg(target_os = "android")]
                    android::watch_opened(app.handle(), core.clone());
                    std::thread::spawn(move || {
                        // Antes dos downloads: nada da obra que não pode aparecer volta a baixar.
                        if let Err(e) = extensions::sources::forget_blocked(&core, &core.db()) {
                            log::warn!("limpeza de obras bloqueadas falhou: {e}");
                        }
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
            commands::take_opened_comic,
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
            commands::open_improvement_form,
            commands::open_project_overview,
            commands::repositories_list,
            commands::repositories_recommended,
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
            commands::source_verify,
            commands::series_library,
            commands::series_set_favorite,
            commands::series_set_collections,
            commands::series_add_to_collection,
            commands::series_remove_from_collection,
            commands::series_set_read,
            commands::series_forget,
            commands::series_set_chapter_sort,
            commands::series_set_reader_mode,
            commands::chapter_open,
            commands::chapter_save_progress,
            commands::chapter_reset_progress,
            commands::chapter_acknowledge,
            commands::chapter_reading_start,
            commands::chapter_reading_touch,
            commands::chapter_reading_end,
            commands::chapter_reading_complete,
            commands::chapters_set_read,
            commands::chapters_mark_previous_read,
            commands::series_refresh_following,
            commands::online_stop,
            commands::series_following,
            commands::comic_origin,
            commands::downloads_list,
            commands::downloads_enqueue,
            commands::download_pause,
            commands::download_resume,
            commands::download_cancel,
            commands::download_retry,
            commands::downloads_clear_finished,
            commands::notifications_list,
            commands::notifications_unread,
            commands::notifications_set_read,
            commands::notifications_mark_all_read,
            commands::notifications_remove,
            commands::notifications_clear_read,
            commands::notification_target,
            commands::notifications_native_status,
            commands::notifications_native_test,
            commands::telemetry_status,
            commands::telemetry_set_consent,
            commands::telemetry_reset_id,
            commands::telemetry_forget,
            commands::telemetry_record,
            commands::telemetry_ready,
            commands::mal_status,
            commands::mal_connect,
            commands::mal_connect_cancel,
            commands::mal_disconnect,
            commands::mal_series,
            commands::mal_search,
            commands::mal_link,
            commands::mal_unlink,
            commands::mal_reset_link,
            commands::mal_retry,
            commands::mal_discard_failed,
            commands::mal_open,
        ])
        .build(context)
        .expect("erro ao executar o Prancha")
        .run(|app, event| {
            // Fechamento normal: guarda no disco o que a telemetria ainda não enviou e marca a saída limpa. Sem rede.
            if let RunEvent::Exit = event {
                if let Some(core) = app.try_state::<Arc<Core>>() {
                    if let Err(e) = core.telemetry.on_exit(&core.db()) {
                        log::warn!("telemetria no fechamento: {e}");
                    }
                }
            }
        });
}
