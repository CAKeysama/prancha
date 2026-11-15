//! O Prancha no Android: os mesmos comandos do desktop (crates/prancha-app), com a configuração deste app.

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    prancha_app::run(tauri::generate_context!())
}
