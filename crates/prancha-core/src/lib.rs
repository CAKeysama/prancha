//! Prancha — núcleo sem Tauri: biblioteca, formatos, páginas, leitura guiada, extensões e integrações.
//! Os comandos que a interface chama ficam em `prancha-app`.

pub mod core;
pub mod covers;
pub mod db;
pub mod error;
pub mod extensions;
pub mod formats;
pub mod guided;
pub mod import;
pub mod library;
pub mod location;
pub mod mal;
pub mod natural;
pub mod notifications;
pub mod pages;
pub mod telemetry;
pub mod text;
