fn main() {
    // O mesmo `cfg(desktop)` / `cfg(mobile)` que o tauri-build cria no aplicativo; aqui não há tauri.conf.json para ele ler.
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let mobile = os == "android" || os == "ios";
    println!("cargo:rustc-check-cfg=cfg(desktop)");
    println!("cargo:rustc-check-cfg=cfg(mobile)");
    println!("cargo:rustc-cfg={}", if mobile { "mobile" } else { "desktop" });
    println!("cargo:rerun-if-changed=build.rs");
}
