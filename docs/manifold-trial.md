# Manifold trial (14 November 2026)

Step 0 of `docs/superpowers/plans/2026-11-14-manifold-core.md` in Arqiflow: can `manifold-csg` 0.4.2 (Manifold 3.5.5) be built
into the engine's browser build (wasm32-unknown-unknown, `unstable-wasm-uu`) and run, with the engine built the way
`scripts/engine-build.mjs` builds it? Worktree `bimcraft-manifold`, branch `geom/manifold` from `ce5521de`.

## Result: it works

- Native: `cargo test -p bimcraft-geom solid_trial -j 4` passes (a box minus a cylinder: volume 803965.7193408787, 272
  triangles; the analytic volume of the 64-gon is 803965.7193408788).
- Browser build: `manifold-csg` alone, then `bimcraft-web` (crates/web) and the app `bimcraft-webapp` (apps/bimcraft-web), each in
  cargo's `release` profile and the fork's `wasm` profile, then `wasm-bindgen 0.2.129 --target web` and `wasm-opt` with
  engine-build.mjs's flags (`npx -p binaryen@125.0.0 wasm-opt -O2 --strip-debug ... `): all four build.
- Node: `web-demo/manifold-trial-node.cjs` calls the exported `manifold_trial()` on the `--target nodejs` glue, with the
  unoptimised wasm and with the wasm-opt'd one (release and wasm profiles): volume and triangle count as above, 18-27 ms.
- Chrome 153 (Playwright's Chromium, headed): `web-demo/manifold-trial.html` loads `pkg/bimcraft_web.js` and calls
  `manifold_trial()`: same numbers, engine loaded in 93 ms, the cut 15 ms. Screenshot: `docs/manifold-trial-browser.png`.
  The app (`web-demo/app.html`, wasm profile, wasm-opt'd, with the dependency linked) also loads and shows the sample house.
- Nothing in the C++ needed switching to Emscripten or npm.

What is in the branch: `manifold-csg` as a target-specific dependency of `crates/geom` (desktop: default features minus
`parallel`; wasm32-unknown-unknown: `--no-default-features` plus `unstable-wasm-uu`), `geom::solid::solid_trial` and its test,
`crates/web`'s exported `manifold_trial()`, the two web-demo files above, and `vendor/manifold-csg-sys` (a copy of the sys crate
with two Windows fixes, wired in by `[patch.crates-io]`).

## What the machine needs (Windows, nothing system-wide)

| Piece | Where | Notes |
|---|---|---|
| LLVM 20.1.8 | `C:\Users\al_mi\tools\llvm-20` | `clang+llvm-20.1.8-x86_64-pc-windows-msvc.tar.xz` from github.com/llvm/llvm-project (940 MB). Has `clang`, `clang++`, `wasm-ld`, `llvm-ar`, wasm32 target. |
| libc++ headers | `C:\Users\al_mi\tools\libcxx-20.1.8\libcxx-20.1.8.src\include` | `libcxx-20.1.8.src.tar.xz` from the same release (4 MB). **The Windows LLVM archive has no libc++ headers** (no `include/c++/v1`). The crate ships its own `__config_site` and `__assertion_handler`, so the source tree's `include/` is enough. |
| cmake 3.31, ninja 1.12 | Visual Studio 2022's bundled ones | `...\Common7\IDE\CommonExtensions\Microsoft\CMake\CMake\bin` and `...\CMake\Ninja` |
| MSVC | Visual Studio 2022 (vcvars64) | for the host build scripts and the native build; run cargo from PowerShell or a vcvars shell, never Git Bash (its GNU `link`). |

Environment for a wasm build (what the scratch scripts set; `engine-build.mjs` takes `process.env` as its base, so these set in
the shell before `npm run engine:build` reach cargo unchanged):

```
PATH                         += C:\Users\al_mi\tools\llvm-20\bin; <VS cmake bin>; <VS Ninja dir>
CMAKE_GENERATOR              = Ninja
WASM_CXX_SHIM_LLVM_BIN_DIR   = C:\Users\al_mi\tools\llvm-20\bin
WASM_CXX_SHIM_LIBCXX_HEADERS = C:/Users/al_mi/tools/libcxx-20.1.8/libcxx-20.1.8.src/include
MANIFOLD_CSG_NO_SCCACHE      = 1          (optional, see below)
CMAKE_BUILD_PARALLEL_LEVEL   = 4
RUSTFLAGS                    = --cfg getrandom_backend="wasm_js"   (engine-build.mjs already sets it)
cargo build --profile <release|wasm> -p <bimcraft-web|bimcraft-webapp> --target wasm32-unknown-unknown -j 4
```

Native needs only the MSVC environment and cmake (as the bake-off's `build.ps1`), `MANIFOLD_CSG_NO_SCCACHE=1` optional.
Arqiflow's `scripts/rust-env.mjs` could add the LLVM and Ninja directories and these variables itself (not done: no Arqiflow files
were touched).

## What broke, and the workaround (every one)

1. **No libc++ headers in the Windows LLVM archive.** `build.rs` stops with "Could not find an LLVM install with libc++ headers".
   Workaround: the libc++ source tarball's `include/` and `WASM_CXX_SHIM_LIBCXX_HEADERS`.
2. **`find_llvm()` in `build/wasm.rs` tests `bin/clang++` without `.exe`**, which is false on Windows, so the probe ladder finds
   nothing; setting `WASM_CXX_SHIM_LIBCXX_HEADERS` skips the probe and takes `clang++` from `where` (LLVM's `bin` must be on PATH).
   The shim's `toolchain-wasm32.cmake` has the same `EXISTS ".../clang"` test, and falls back to the PATH lookup the same way.
   (`llvm-ar` likewise resolves from PATH.) No source change: PATH covers it.
3. **cmake's default generator under vcvars is NMake**, which fails in FetchContent's populate step
   (`NMAKE : fatal error U1073: don't know how to make 'manifold-populate-prefix/src/manifold-populate-stamp/manifold-populate-patch-info.txt'`).
   Workaround: `CMAKE_GENERATOR=Ninja`.
4. **Windows paths with backslashes are eaten** in `wasm32-uu/CMakeLists.txt`'s `"SHELL:-isystem ${DIR}"` options (CMake splits
   `SHELL:` like a Unix shell): `-isystem C:Usersal_mi.cargoregistrysrc...` and then `fatal error: 'cstdint' file not found`
   in Clipper2. Fix in the vendored copy (`build/wasm.rs`: `cmake_path()` gives the `-D` paths forward slashes).
5. **A path with a space splits the same options in two** (`-isystem G:/Software Development/...` then
   `clang++: error: no such file or directory: 'Development/bimcraft-manifold/vendor/...'`); this repo and Arqiflow's
   `target-wasm` live under `G:\Software Development\`. Fix in the vendored copy: the four `SHELL:` options are quoted
   (`wasm32-uu/CMakeLists.txt`). Checked with a target dir `D:/cargo-targets/manifold space test/target wasm`: builds.
   Fixes 4 and 5 are marked "BIMCraft's patch" in the files; both would be worth sending upstream
   (github.com/zmerlynn/manifold-csg). Without the patch, a registry copy of the crate fails on this PC, so the patch is not
   optional here; on Linux it is not needed.
6. **sccache is picked up automatically** when it is on the PATH (`cmake_launcher_args`); it worked, but a "cold" timing is then
   not cold, so `MANIFOLD_CSG_NO_SCCACHE=1` was set for the measurements.

Nothing else: the C++ compiled and linked as is (Manifold 3.5.5 through the shim v0.5.0, which was tested against an older
Manifold), `wasm-bindgen` found no unresolved imports (the module imports nothing from the C++ side), and `wasm-opt` kept it
working.

## Sizes (engine-build.mjs's wasm-opt flags; bytes; ce5521de against this branch)

| Build | raw before | raw after | gzip -9 | brotli 11 |
|---|---|---|---|---|
| bimcraft-web, release (the server's host) | 22,357,129 | 22,665,273 (+308 kB, +1.4%) | 8.20 -> 8.33 MB (+123 kB) | 5.45 -> 5.54 MB (+86 kB) |
| bimcraft-web, wasm profile | 17,164,522 | 17,469,992 (+305 kB, +1.8%) | 6.23 -> 6.34 MB (+116 kB) | 4.33 -> 4.41 MB (+80 kB) |
| bimcraft-webapp, release | 36,582,931 | 36,596,367 (+13 kB) | +16 kB | +12 kB |
| bimcraft-webapp, wasm profile (what browsers download) | 28,740,920 | 28,753,274 (+12 kB) | 10.19 -> 10.20 MB | 6.62 -> 6.63 MB (+9 kB) |

Before wasm-opt (wasm-bindgen's output): web release 30,939,203 -> 31,311,660, web wasm 18,272,718 -> 18,597,487.

The app grows by only 12 kB because nothing in it reaches Manifold yet (wasm-ld drops what is unreferenced). Once the model
calls `geom::solid`, the app takes what `bimcraft-web` took: about +0.3 MB raw, +80-90 kB brotli (1.3% of the download).

## Build times (this PC, 5900X, `-j 4`, `CMAKE_BUILD_PARALLEL_LEVEL=4`)

Manifold's own C++ (three git clones, cmake configure, about 28 compile steps, `MANIFOLD_CSG_NO_SCCACHE=1`):

| | cold (clean OUT_DIR, network) | warm |
|---|---|---|
| wasm32-unknown-unknown, `manifold-csg` alone | 58-60 s | nothing to do: 0 s (5 s of cargo and vcvars); a rerun of `build.rs` in the same OUT_DIR (a changed env var) 11 s |
| native MSVC, `bimcraft-geom` test build | 77-80 s (with the Rust crates around it) | 8 s |

The C++ is rebuilt only when cargo deletes the crate's OUT_DIR (`cargo clean`, a new target dir, a changed version or path of
the crate), never on an ordinary rebuild of the engine.

Whole engine builds (cargo only, cold target dir each; the two sets ran partly at the same time as other builds, so the
difference is noise):

| | before (ce5521de) | after (with manifold, cold C++ included) |
|---|---|---|
| bimcraft-web release | 4m56 | 5m08 |
| bimcraft-web wasm | 6m14 | 5m16 |
| bimcraft-webapp release | 6m44 | 5m51 |
| bimcraft-webapp wasm | 6m03 | 6m03 |

Script totals (cargo, wasm-bindgen, wasm-opt through npx): before 655 s (web wasm), 1012 s (app release), 849 s (app wasm);
after 775 s (web release), 590 s (web wasm), 1066 s (app release), 800 s (app wasm). The target directories were
`D:\cargo-targets\manifold-trial\wasm-after` (2.8 GB for the four builds).

## Offline

- A rebuild in a warm target dir needs no network, even when the crate's build script reruns (tested with `CARGO_NET_OFFLINE=true`
  and `HTTPS_PROXY` pointing at a closed port: the rerun reconfigured cmake from the cached sources and finished in 11 s).
- A **clean build always needs github.com**: `build.rs` clones `wasm-cxx-shim` v0.5.0 (`git clone --depth 1`), and cmake's
  FetchContent clones Manifold v3.5.5 and Clipper2 (2.0.1 + 12 commits); the native build clones Manifold. Tested offline: stops at
  `fatal: unable to access 'https://github.com/zmerlynn/wasm-cxx-shim.git/'`. Cargo's own `CARGO_NET_OFFLINE` does not cover this.
- To make a clean **native** build work offline, `MANIFOLD_CSG_LIB_DIR` (and `MANIFOLD_CSG_LIB_KIND`): a directory holding
  prebuilt `manifoldc`, `manifold` and, for `static`, `Clipper2` (`libmanifoldc.a`, `libmanifold.a`, `libClipper2.a`; on MSVC
  `manifoldc.lib`, `manifold.lib`, `Clipper2.lib`). The build script then skips the clones and cmake altogether and only
  emits the link lines (the FFI is hand-written, so no headers are needed). `MANIFOLD_CSG_LIB_KIND` is `dylib` (the default: a
  shared libmanifold records its own Clipper2 and libstdc++ dependencies) or `static`. To make the libraries, build Manifold
  at the commit pinned in `vendor/manifold-csg-sys/build.rs` (`MANIFOLD_VERSION`) with cmake:
  `-DMANIFOLD_CBIND=ON -DMANIFOLD_TEST=OFF -DMANIFOLD_PAR=OFF -DMANIFOLD_CROSS_SECTION=ON -DMANIFOLD_USE_BUILTIN_CLIPPER2=ON
  -DBUILD_SHARED_LIBS=OFF -DCMAKE_POSITION_INDEPENDENT_CODE=ON -DCMAKE_BUILD_TYPE=Release`; the three archives are in
  `build/bindings/c`, `build/src` and `build/_deps/clipper2-build` (they are what a normal build leaves in
  `target/<profile>/build/manifold-csg-sys-*/out/build`). Set both variables for the whole build, e.g.
  `MANIFOLD_CSG_LIB_DIR=/opt/manifold/lib MANIFOLD_CSG_LIB_KIND=static cargo build -p bimcraft`. Wasm has no such switch: it
  would need the vendored crate patched to pass `FETCHCONTENT_SOURCE_DIR_*` and a local shim.
- **What a clean build clones, and the pins** (`vendor/manifold-csg-sys`, so the C++ is not at the mercy of a moved tag):
  Manifold by **commit** `32271a1a999ce721ffaf5ab10e9746ceeb696547` (v3.5.5; `MANIFOLD_VERSION` in `build.rs`, used by the
  native clone and passed to the shim's helper for wasm); its Clipper2 is pinned by commit inside Manifold's own
  `cmake/manifoldDeps.cmake` (`46f639177fe418f9689e8ddb74f08a870c71f5b4`, "2.0.1 + 12 commits"), so the one pin covers it; the
  wasm shim by commit `a156e60fcc324f6ce7b7864e39a3de770809423c` (v0.5.0; `WASM_CXX_SHIM_COMMIT` in `build/wasm.rs`, fetched by
  SHA with `git fetch --depth 1 origin <sha>`). To move a pin, change the constant, and the comment that says which release it is.

## Linux (not built)

- The Arqiflow deploy image does not build the engine: `server/Dockerfile` downloads the finished wasm from the fork's release
  (`scripts/engine-files.mjs`). So the **image needs nothing for Manifold**; the toolchain matters only where
  `npm run engine:build` runs (this PC) and for the fork's own CI.
- A Linux wasm build would need: `clang-20 lld-20 llvm-20 libc++-20-dev libc++abi-20-dev` (Debian/Ubuntu from apt.llvm.org; the
  distro's own clang is older on bookworm, `python:3.12-slim-bookworm`), `cmake` >= 3.20, `git`, `make` or `ninja`, and network
  access to github.com. `build.rs` finds `/usr/lib/llvm-20/bin` by itself (else `WASM_CXX_SHIM_LLVM_BIN_DIR`); the libc++ headers
  come from `/usr/lib/llvm-20/include/c++/v1` (it warns if that is a symlink to `/usr/include`; then set
  `WASM_CXX_SHIM_LIBCXX_HEADERS`). Neither fix 4 nor fix 5 applies on Linux, and no `CMAKE_GENERATOR` is needed.
- A Linux native build (the fork's CI runs `cargo test --workspace` on three systems; the release job builds the desktop and CLI):
  `cmake`, `git`, a C++17 compiler (g++ or clang++) and network access, or `MANIFOLD_CSG_LIB_DIR`. No TBB, because `parallel` is off.
  Every native build of the fork, the desktop included, now compiles Manifold once (about 1.5 minutes cold). The binaries now
  link libstdc++ besides glibc and libgcc_s, so the deb and rpm packages depend on it (`packaging/linux/nfpm.yaml`).

## The release workflow (`.github/workflows/release.yml`), reviewed; not run here

- **Windows, Linux x86_64 and aarch64 (native runners):** nothing to add. The runners have cmake, git and a C++17 compiler (VS 2022,
  g++ 11 on ubuntu-22.04), and the C++ is rebuilt on every clean checkout of the runner (about 1.5 minutes).
- **Linux riscv64 (cross, on ubuntu-24.04): needed `g++-riscv64-linux-gnu`.** The sys crate drives `cmake` itself and used to
  pass it nothing about the target, so a cross build made x86_64 objects. It is now (a patch in `vendor/manifold-csg-sys/build/build.rs`,
  `cross_args`) given `CMAKE_SYSTEM_NAME`, `CMAKE_SYSTEM_PROCESSOR` and the compilers named by `CC_riscv64gc_unknown_linux_gnu` and
  `CXX_riscv64gc_unknown_linux_gnu` (set in the job, with `g++-riscv64-linux-gnu` in its apt line). The riscv64 deb needs libstdc++6 >= 13.
  Not verified: run the workflow once by `workflow_dispatch` before the next tag.
- **macOS universal (arm64 runner building x86_64):** the same defect, and worse (the x86_64 slice would link arm64 objects): `cross_args`
  now passes `CMAKE_OSX_ARCHITECTURES` (arm64 or x86_64) for every Apple target. Not verifiable here; the same dispatch run tells.
- **Arch packages:** `arch-package.sh` only repackages the tarball (no `cargo`, no compile), so its container needs neither cmake nor git.
- **Wasm:** neither workflow builds the engine for the browser (`ci.yml`'s only wasm build is the sample plugin, which has no solid
  kernel); that is `scripts/engine-build.mjs` on the owner's PC, and the Arqiflow image downloads the result. If a job is ever added:
  `clang-20 lld-20 llvm-20 libc++-20-dev libc++abi-20-dev` from apt.llvm.org, `cmake`, `ninja-build`, `git`, as above.
- Unsupported: `wasm32-wasip1`/`wasip2` would take the native path and cannot build (the geom crate's cfg sends every wasm target but
  `wasm32-unknown-unknown` there); no target in the workspace is one.

## Caveats to carry into step 1

- **`parallel` is off** on both targets: Manifold runs single-threaded natively (no TBB). Turning it on natively means building
  oneTBB through cmake (not tried); it does not exist for wasm32-unknown-unknown.
- **C++ exceptions abort in the browser** (`-fno-exceptions`; implicit STL throws, e.g. out of memory, become a wasm trap); in the
  engine that is the trap the page already handles by reloading. Manifold itself returns status codes (`from_meshgl64` gives
  `Result`) instead of throwing, which step 1's "refused, with the reason" relies on; not exercised in this trial.
- The wasm route is the crate's own `unstable-wasm-uu`, labelled provisional; the pins are the crate's (Manifold v3.5.5 from the
  host pin, wasm-cxx-shim v0.5.0, tested by upstream against Manifold v3.5.0). The C++ sources are cloned at build time, outside
  Cargo.lock, so they are pinned by commit in the vendored crate (see Offline).
- `unsafe_code = "forbid"` and the clippy lints hold: only the dependency has unsafe code; `solid_trial` is safe Rust.
- The first wasm build of a fresh clone has the network and the LLVM set-up above as prerequisites; `engine-build.mjs` itself
  needs no change, only its environment.
