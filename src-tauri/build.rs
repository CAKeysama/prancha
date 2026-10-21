fn main() {
    // A biblioteca UnRAR (C++) usa funções do registro e de token do Windows.
    #[cfg(windows)]
    println!("cargo:rustc-link-lib=advapi32");
    tauri_build::build()
}
