//! Compila as extensões de `extensions/` para os testes: os componentes são de verdade, não simulados.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::{Mutex, OnceLock};

pub fn target_dir() -> PathBuf {
    let base = std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target"));
    base.join("extensions-wasm")
}

/// Bytes do componente do pacote indicado. Compila uma vez por execução dos testes.
pub fn component(package: &str) -> Vec<u8> {
    static BUILT: OnceLock<Mutex<HashMap<String, Vec<u8>>>> = OnceLock::new();
    let mut built = BUILT.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
    built.entry(package.to_string()).or_insert_with(|| build(package)).clone()
}

fn build(package: &str) -> Vec<u8> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../extensions/Cargo.toml");
    let target = target_dir();
    let status = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .args(["build", "--release", "--target", "wasm32-unknown-unknown", "--manifest-path"])
        .arg(&manifest)
        .args(["-p", package])
        .env("CARGO_TARGET_DIR", &target)
        .status()
        .expect("cargo não abriu");
    assert!(status.success(), "a compilação de {package} falhou; o alvo wasm32-unknown-unknown está instalado?");
    let file = target.join("wasm32-unknown-unknown/release").join(format!("{}.wasm", package.replace('-', "_")));
    let module = std::fs::read(&file).unwrap_or_else(|e| panic!("{}: {e}", file.display()));
    wit_component::ComponentEncoder::default().module(&module).unwrap().validate(true).encode().unwrap()
}
