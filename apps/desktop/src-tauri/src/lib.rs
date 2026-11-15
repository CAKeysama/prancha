//! Prancha para desktop: o contexto do Tauri (tauri.conf.json, ícones e a interface em ../dist) e o ponto de entrada.

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    prancha_app::run(tauri::generate_context!())
}
