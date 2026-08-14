# Simple Table

Simple Table is a pure-Rust spreadsheet editor built with Dioxus 0.7.10. One
codebase targets desktop, iOS, Android, and the Web. The Web build uses Axum
SSR followed by Dioxus hydration; workbook state remains in a browser-side Rust
Web Worker and IndexedDB.

## Features

- XLSX and CSV open, edit, search, and save flows
- Cell, sheet, row, column, image, formula, undo/redo, and dirty-state support
- Dioxus Desktop and Mobile native packages
- Stateless Axum SSR with a hydrated Web client in one deployable binary
- Official Dioxus Primitives controls and official Lucide icon data

## Prerequisites

- Current stable Rust
- `wasm32-unknown-unknown` target
- Dioxus CLI 0.7.10:
  `cargo install dioxus-cli --version 0.7.10 --locked`
- wasm-bindgen CLI 0.2.127:
  `cargo install wasm-bindgen-cli --version 0.2.127 --locked`
- Android Studio/NDK for Android and Xcode for iOS

## Development

```bash
cargo xtask desktop
cargo xtask ios
cargo xtask android
cargo xtask web
```

`web` launches the SSR and hydration pair for development and builds the
browser Worker first. The mobile tasks launch the active simulator or connected
device and accept Dioxus options such as `--device`. Set `DIOXUS_CLI` when the
required `dx` binary is not in `PATH`.

## Verification And Builds

```bash
cargo fmt --all -- --check
cargo xtask check
cargo xtask test
cargo xtask bundle
```

The `desktop`, `mobile`, Web hydration, SSR, and Worker targets are checked
separately. `cargo xtask bundle` is the only Web deployment build. It writes a
self-contained SSR executable to `target/release/simple-table-web`; generated
JavaScript, Wasm, CSS, fonts, the favicon, and Worker files are embedded in the
binary and no adjacent `public/` directory is required.

```bash
docker build -t simple-table .
docker run --rm -p 8080:8080 simple-table
```

## Structure

```text
Cargo.toml                         Virtual workspace and dependency policy
Dioxus.toml                        Dioxus CLI and bundle configuration
apps/simple-table/                 Cross-platform Dioxus app and assets
apps/simple-table-web-worker/      Browser Worker and IndexedDB adapter
apps/simple-table-web-server/      Development and embedded production SSR
crates/simple-table-protocol/      Bounded request/reply contract
crates/simple-table-engine/        Document, operations, state, I/O, adapters
xtask/                             Development, verification, and bundle tasks
```

Rust 2018-style module roots (`components.rs`, `ports.rs`, and the engine's
responsibility roots) are used throughout; the repository contains no `mod.rs`.
No JavaScript or TypeScript source is checked in. Dioxus and wasm-bindgen
generate the JavaScript and Wasm needed by browsers under `target/` during Web
builds. The official `dioxus-primitives` source is pinned to an audited Git
revision because it does not currently have a usable crates.io release.

See [docs/architecture.md](docs/architecture.md) for ownership and persistence
details.

## License

MIT
