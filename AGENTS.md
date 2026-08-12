# AGENTS.md

Guidance for coding agents working in this repository.

## Project

Simple Table is a pure-Rust workspace built with Dioxus 0.7.10. The root
package owns the cross-platform app and `backend/` contains the independent
`simple-table-engine` crate. Desktop and mobile call the engine directly. Web
is rendered by stateless Axum SSR, hydrates in the browser, and performs
workbook operations in a Rust Web Worker. Do not introduce Tauri, Vue,
TypeScript, Node, checked-in JavaScript, or server-side workbook storage.

## Commands

```bash
cargo xtask desktop       # Dioxus Desktop development
cargo xtask ios           # iOS simulator/device development
cargo xtask android       # Android emulator/device development
cargo xtask web           # SSR + browser hydration development
cargo xtask bundle        # production embedded SSR binary

cargo fmt --all -- --check
cargo xtask check          # desktop + SSR + Web Wasm + Worker Wasm
cargo test --workspace
```

The target features are mutually exclusive. Build and lint them separately;
never use `--all-features` for this package.

Mobile checks require their platform toolchains:

```bash
cargo clippy --target aarch64-apple-ios-sim \
  --no-default-features --features mobile --all-targets -- \
  -Dwarnings -Dclippy::redundant_clone -Dclippy::clone_on_copy -Dclippy::implicit_clone
cargo clippy --target aarch64-linux-android \
  --no-default-features --features mobile --all-targets -- \
  -Dwarnings -Dclippy::redundant_clone -Dclippy::clone_on_copy -Dclippy::implicit_clone
```

## Architecture

- `backend/src/protocol.rs` owns serializable cross-target requests and replies.
- `backend/src/lib.rs` is the engine crate root. The remaining files under
  `backend/src/` retain the existing document, operations, state, I/O,
  application, and adapter responsibility boundaries.
- `src/lib.rs`, `src/components.rs`, and `src/components/` own Dioxus routes,
  views, and state coordination.
- `src/ports.rs` and `src/ports/` isolate platform behavior. Components must
  not bypass ports to mutate backend state.
- `src/web_worker.rs` owns the browser `CoreFacade` from `simple-table-engine`
  and IndexedDB. Workbook
  bytes never travel to the SSR server.
- `src/web_server.rs` owns the production Axum router and embedded Web assets.
- `src/xtask.rs` owns repeatable multi-stage builds.

Do not add `mod.rs`; declare a directory module with the adjacent modern module
file, for example `src/components.rs` plus `src/components/editor.rs`.

Switch, Tabs, and Toolbar come directly from the official
`dioxus-primitives` package. The dependency is pinned to an audited upstream
commit; app-specific appearance remains in `assets/main.css`. Local UI code is
limited to the Lucide icon adapter in `src/ui/`.

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
- Keep platform behavior behind modules in `src/ports/`.
- Prefer official Dioxus Primitives and Lucide assets for controls. Do not copy
  upstream component implementations into the app.
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
