# AGENTS.md

Guidance for coding agents working in this repository.

## Project

Simple Table is a pure-Rust virtual workspace built with Dioxus 0.7.10.
`apps/` contains the cross-platform UI and Web delivery binaries; `crates/`
contains the protocol and spreadsheet engine. Desktop and mobile call the
engine directly. Web is rendered by stateless Axum SSR, hydrates in the
browser, and performs workbook operations in a Rust Web Worker. Do not
introduce Tauri, Vue, TypeScript, Node, checked-in JavaScript, or server-side
workbook storage.

## Commands

```bash
cargo xtask desktop       # Dioxus Desktop development
cargo xtask ios           # iOS simulator/device development
cargo xtask android       # Android emulator/device development
cargo xtask web           # SSR + browser hydration development
cargo xtask bundle        # production embedded SSR binary

cargo fmt --all -- --check
cargo xtask check          # desktop + SSR + Web Wasm + Worker Wasm
cargo xtask test           # protocol + engine + app feature test matrix
```

The target features are mutually exclusive. Build and lint them separately;
never use `--all-features` for this package.

Mobile checks require their platform toolchains:

```bash
cargo clippy -p simple-table --target aarch64-apple-ios-sim \
  --no-default-features --features mobile --all-targets -- \
  -Dwarnings -Dclippy::redundant_clone -Dclippy::clone_on_copy -Dclippy::implicit_clone
cargo clippy -p simple-table --target aarch64-linux-android \
  --no-default-features --features mobile --all-targets -- \
  -Dwarnings -Dclippy::redundant_clone -Dclippy::clone_on_copy -Dclippy::implicit_clone
```

## Architecture

- `crates/simple-table-protocol/src/lib.rs` owns serializable cross-target
  requests and replies.
- `crates/simple-table-engine/src/lib.rs` is the engine crate root. Its sibling
  modules retain the document, operations, state, I/O, application, and adapter
  responsibility boundaries.
- `crates/simple-table-components/` is the only UI-library boundary. Its
  `src/components/` tree and official theme asset are CLI-generated upstream
  source; `src/lib.rs` is the project facade.
- `apps/simple-table/src/lib.rs`, `components.rs`, and `components/` own Dioxus
  routes, views, and state coordination.
- `apps/simple-table/src/ports.rs` and `ports/` isolate platform behavior.
  Components must not bypass ports to mutate engine state.
- `apps/simple-table-web-worker/src/main.rs` owns the browser `CoreFacade` and
  IndexedDB. Workbook bytes never travel to the SSR server.
- `apps/simple-table-web-server/src/main.rs` owns development and embedded
  production Axum SSR.
- `xtask/src/main.rs` owns repeatable multi-stage builds.

Do not add `mod.rs` to project-owned code; declare a directory module with the
adjacent modern module file, for example `src/components.rs` plus
`src/components/editor.rs`. The official generated component tree is the only
exception and retains its upstream module layout.

Finished controls come from the official Dioxus Components registry and icons
come from the official `dioxus-icons` crate. Both are isolated behind
`simple-table-components`; the app must not depend on or import
`dioxus-primitives`, `dioxus-icons`, or other icon packages directly. Keep
generated sources unchanged, app composition in `apps/simple-table/src/ui.rs`,
and app appearance in `apps/simple-table/assets/main.css`. Refresh generated
sources only with `cargo xtask components`.

## Editor Invariants

- Rust backend state is authoritative.
- UI dirty state combines the backend content hash with pending debounced edits.
- Flush pending edits before save, undo/redo, search, sheet-dependent actions,
  and navigation.
- Row and column dimensions are document edits and participate in dirty state,
  undo, and redo.
- Web recovery is written to IndexedDB, never to the SSR process.
- Keep `CoreFacade` requests bounded and preserve revision checks.

## Code Guidelines

- Use `AppError` in the backend and `AppErrorDto` across the protocol boundary.
- Put business logic in the existing engine responsibility module, not views.
- Keep platform behavior behind modules in `apps/simple-table/src/ports/`.
- Prefer official styled Dioxus Components and official Dioxus Lucide icons for
  controls. Do not copy or customize upstream component implementations in the
  app or generated component tree.
- Keep JavaScript and TypeScript out of source control. The `web` and `bundle`
  tasks generate browser Worker binding glue under `target/generated-public/`.
- `cargo xtask bundle` is the only Web deployment build. It must produce the
  self-contained `target/release/simple-table-web`; do not publish a standalone
  CSR site or a server with an adjacent `public/` directory.
- Run `cargo fmt` after Rust edits.
- Add focused tests for changed engine behavior or shared interaction logic.

## Versioning

The shared package version is defined once under `[workspace.package]` in root
`Cargo.toml`. Tags use a `v` prefix, for example `v0.12.0`.
