# ADR 0001: Wasmtime como runtime de extensões

**Estado:** aceita (2026-11-02)

## Contexto

O Prancha precisa executar código de terceiros que consulta fontes de leitura. Esse código não pode tocar no
sistema de arquivos, no banco, nos processos ou na rede por conta própria, e uma extensão ruim não pode derrubar
o aplicativo nem travar a leitura. As extensões do Mihon são APKs Android e não rodam neste runtime: a [ADR 0002](0002-repositorios-do-mihon-pelo-suwayomi.md) trata delas.

## Decisão

Extensões são **componentes WebAssembly** executados pelo **Wasmtime 48**, com o contrato em WIT
(`extensions/wit/world.wit`) e **sem WASI**.

| Tema | Decisão | Motivo |
|---|---|---|
| Runtime | `wasmtime` 48.0.x | Múltiplos de 12 são LTS, com 24 meses de correções. As versões 49 e 50 têm só 2 meses. `x86_64-pc-windows-msvc` é Tier 1. |
| Rust mínimo | 1.95 | Exigência do Wasmtime 48. O projeto compila com a versão estável (1.99). |
| Recursos do crate | `cranelift`, `runtime`, `component-model`, `std` | O resto (GC, threads, cache em disco, perfil, depuração, `wat`) não é usado e só aumentaria o executável. |
| Compilador | Cranelift | O ganho de tamanho do Winch é de só 0,8 MB, e a compilação de 37 ms de uma extensão pequena acontece uma vez por sessão. |
| Contrato | WIT e Component Model | Interface formal e versionada, com tipos ricos e validação feita pelo runtime na fronteira. |
| Alvo das extensões | `wasm32-unknown-unknown` + `wit-component` | Gera um componente cujo único import é o host do Prancha. `wasm32-wasip2` importaria WASI. |
| Isolamento | Sem WASI | Sem o import, não há como pedir arquivo, relógio, ambiente ou socket. Um componente que os importe nem carrega. |
| Modo de execução | Chamadas síncronas em threads de trabalho, instância nova por chamada | As chamadas são curtas e o estado fica fora da instância. O modo assíncrono exigiria o recurso `async`, fibras e pilhas extras sem ganho aqui, porque a concorrência já é limitada. A rede do host roda no runtime Tokio do app, e a thread espera com cancelamento. |
| Interrupção | Fuel e epoch | Fuel limita instruções. Epoch limita tempo e permite cancelar. Os dois viram erro e não afetam o resto do app. |

## Alternativas

- **Outro runtime:** não houve incompatibilidade técnica com o Wasmtime, então nenhum outro foi avaliado.
- **`wasm32-wasip2` com WASI negado por política:** deixaria a superfície do WASI no linker e obrigaria a
  implementar interfaces que o Prancha não quer oferecer.
- **Winch:** é um compilador de base, sem as otimizações do Cranelift. Compila mais rápido, mas a medição abaixo não
  mostra ganho que compense, e as extensões analisam HTML e JSON a cada chamada.
- **Executar APKs do Mihon:** exigiria um ambiente Android, e continua fora de escopo. O Prancha roda as versões `.jar` delas pelo Suwayomi, fora deste runtime.
- **JavaScript em um WebView:** dividiria o processo com a interface e não ofereceria limites de memória e instruções por chamada.

## Impacto no tamanho

Medido em 2026-11-02 com um executável separado, que só carrega o contrato e chama `list-sources` no componente de
exemplo (24 KB), usando o perfil de release do projeto (`lto = true`, `codegen-units = 1`, `strip = true`). As
durações são de uma única execução e servem só como ordem de grandeza.

| Medição | Cranelift | Winch |
|---|---|---|
| Executável com o runtime e o contrato | 12.596.224 bytes (12,6 MB) | 11.778.048 bytes (11,8 MB) |
| Compilar o componente | 37 ms | 10,5 ms |
| Instanciar e chamar `list-sources` | 91 µs | 122 µs |

Antes das extensões, o `prancha.exe` da 0.2.0 tem 21.607.424 bytes e o instalador NSIS, 8.308.041 bytes. O efeito no
aplicativo completo, com tudo ligado, está em `docs/TEST_REPORT.md`.

## Consequências

- O Rust mínimo do projeto sobe de 1.90 para 1.95.
- Autores de extensões usam Rust com `wasm32-unknown-unknown`, ou qualquer linguagem que gere um componente com o mesmo WIT.
- O CI precisa do alvo `wasm32-unknown-unknown`, porque os testes compilam extensões de verdade.
- Em agosto de 2028 a série 48 sai de suporte: a próxima LTS é a 60.
