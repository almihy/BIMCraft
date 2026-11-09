# bimcraft-web

The engine compiled to WebAssembly for Arqiflow (github.com/almihy/BIMCraft, branch `arqiflow-web`): `Engine` is a
`Session` a page or Node sends commands to, as the CLI and MCP do, plus the reads Arqiflow needs (plans, 3D, IFC, glTF,
links) and the live merge of `core/src/sync.rs` and the file bytes of `core/src/files.rs`. `apps/bimcraft-web` is the
interface's `WebApp`, hosted in a browser tab through `ui-egui/src/host.rs`.

- **Build it from Arqiflow**, not here: `npm run engine:build` there runs `cargo build --release --target
  wasm32-unknown-unknown` with `RUSTFLAGS=--cfg getrandom_backend="wasm_js"`, then `wasm-bindgen` at the version pinned in
  `Cargo.toml` (`=0.2.129`), then `wasm-opt`.
- **No clock in the browser.** Code the engine runs in the browser must not call `std::time::Instant` or `SystemTime`
  directly; it goes behind `cfg(target_arch = "wasm32")`.
- **Keep the API small and JSON.** Every method takes and gives strings or bytes; the command catalogue is the engine's.
