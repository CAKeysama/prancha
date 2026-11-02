//! Tipos e ligações oficiais para escrever extensões do Prancha.
//!
//! Uma extensão implementa [`Guest`] e chama [`export_extension!`]. Ver `docs/extensions/DEVELOPMENT.md`.

// Em um módulo: na raiz do crate, a macro exportada colide com a reexportação que o gerador faz dela.
#[doc(hidden)]
pub mod bindings {
    wit_bindgen::generate!({
        path: "../wit",
        world: "extension",
        pub_export_macro: true,
        export_macro_name: "export_extension",
        default_bindings_module: "prancha_extension_sdk::bindings",
    });
}

pub use bindings::prancha::source::host;
pub use bindings::{Guest, export_extension};
pub use bindings::prancha::source::types::*;
