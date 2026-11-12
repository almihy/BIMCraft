# bimcraft-workspace-hack

No code. Cargo turns on a dependency's features for what a command builds, so without this crate `cargo test -p
bimcraft-core`, `-p bimcraft-ui-egui`, `--workspace` and `cargo build` each built the model's dependencies with other
features, and switching between them rebuilt the model (104 s). The model depends on this crate on the desktop (not in the
browser's build), and it asks for the union of those features, so every command builds the model alike.

- **Keeping it.** When the model rebuilds on switching commands again (a dependency added or updated), compare `cargo
  tree -e normal,build,dev,features --prefix none -f '{p} {f}' --target x86_64-pc-windows-msvc` for `-p bimcraft-core`,
  `-p bimcraft-ui-egui` and `--workspace` over the crates in `cargo tree -p bimcraft-model`, and add what differs. Host
  crates (proc macros' and build scripts' dependencies) go under `build-dependencies` too: cargo resolves the host's
  features apart. Check with the artifact hashes: `cargo test -p bimcraft-core --no-run --message-format=json` and the
  same with `--workspace` should name the same `libbimcraft_model-<hash>.rlib`.
- It was measured on Windows; Linux and macOS have crates of their own (rustix, libc's features) that may want lines here.
