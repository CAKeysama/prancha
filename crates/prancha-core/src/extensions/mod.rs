//! Extensões de fontes de leitura: repositórios, execução em WebAssembly e downloads.

pub mod downloads;
pub mod install;
pub mod minors;
pub mod net;
pub mod reading;
pub mod recommended;
pub mod repository;
pub mod runtime;
pub mod signing;
pub mod sources;
pub mod mihon;
#[cfg(not(target_os = "android"))]
pub mod suwayomi;
// No Android as extensões do Mihon rodam dentro do app, e este módulo ocupa o lugar do Suwayomi.
#[cfg(target_os = "android")]
pub use mihon as suwayomi;
