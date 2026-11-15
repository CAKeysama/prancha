fn main() {
    // A biblioteca UnRAR (C++) usa funções do registro e de token do Windows. Vale o sistema de destino,
    // não o do computador que compila: o build para Android também roda no Windows.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-lib=advapi32");
    }
    tauri_build::build()
}
