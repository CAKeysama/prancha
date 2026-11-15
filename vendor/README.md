# Dependências copiadas

## unrar_sys

Cópia do `unrar_sys` 0.5.8 (crates.io, licença MIT ou Apache 2.0; o código do UnRAR em `vendor/unrar` segue a licença dele, `vendor/unrar/license.txt`). Só o `build.rs` mudou, para compilar para o Android:

- decide pelo sistema de destino (`CARGO_CFG_TARGET_OS`) e não pelo computador que compila, então o build para Android feito no Windows não tenta compilar `isnt.cpp`;
- no Android usa `utimensat` (`UNIX_TIME_NS`), porque `lutimes` não existe na API 24, e liga a biblioteca C++ do NDK.

Quando uma versão publicada do `unrar_sys` resolver isso, apague esta pasta e o `[patch.crates-io]` do `Cargo.toml`.
