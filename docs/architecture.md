# Architecture

Simple Table is a two-package Rust workspace: the root package owns the Dioxus
application and `simple-table-engine` owns the spreadsheet engine and shared
protocol. Platform features select the renderer and delivery adapters; they do
not fork the editor model. The SSR server owns no workbook session or bytes.

```text
Desktop / iOS / Android                  Browser
        |                                  |
        | NativeEditorPort                 | WorkerEditorPort
        v                                  v
  CoreFacade in process              dedicated Rust Worker
        |                                  |
        v                                  v
 simple-table-engine             CoreFacade + IndexedDB

                  Web request
                       |
                       v
             stateless Axum SSR
                       |
                       v
              HTML + hydration assets
```

## Module Ownership

### Protocol

`backend/src/protocol.rs` defines bounded, serializable `EditorRequest` and
`EditorReply` values plus the shared error DTO. The same Rust contract is used
in process and across the browser Worker boundary.

### Engine

`backend/src/lib.rs` declares `simple-table-engine`; implementation files remain
under `backend/src/` with their established responsibility boundaries:

- `document/`: workbook model, backing data, patches, restore, and save
- `ops/`: mutation execution, projections, and operation impact
- `state/`: editor session, history, search data, and dirty tracking
- `io/`: codecs, projections, limits, and atomic desktop file primitives
- `application/`: use-case services and ports
- `adapters/`: codec and search implementations

`CoreFacade::execute` is the application boundary. Platform UI code does not
reach into internal backend state.

### Application And Ports

`src/lib.rs` composes the Dioxus app. `src/components.rs` and
`src/components/` own the rewritten home/editor views, responsive layout, and
pending-edit coordination. `src/ports.rs` and `src/ports/` isolate native,
mobile, and browser behavior.

Switch, Tabs, and Toolbar come directly from the official Dioxus Components
`dioxus-primitives` package. The upstream commit is pinned for reproducible
builds; only app-specific presentation lives in `assets/main.css`.

### Browser Worker

`src/web_worker.rs` is a second binary target in the application package. It
owns a browser-side `CoreFacade` from `simple-table-engine`, serializes requests,
and persists saved and recovery snapshots in IndexedDB. The Worker build is generated into
`target/generated-public/workers/`; no JavaScript or Wasm build output is
checked into the repository.

## Web SSR Lifecycle

1. Axum renders the route and serves Dioxus assets embedded in the executable.
2. The browser hydrates the server HTML.
3. `WorkerEditorPort` starts the module Worker.
4. The Worker initializes the backend and IndexedDB.
5. File bytes go directly from the browser to the Worker.
6. Save creates bytes in the Worker, then stores locally or downloads them.

The server exposes `/healthz` and reads `IP` and `PORT`. It can scale
horizontally because it owns no editor state. Production Web deployment has one
supported shape: `target/release/simple-table-web`; it requires no adjacent
asset directory. Dioxus 0.7's `ServeConfig` requires an index path, so startup
materializes the embedded `index.html` into a private temporary directory. All
served bytes still originate from the executable, and the directory is removed
when the process exits.

## Edit And Save Invariants

Cell edits are held in a 500 ms UI debounce overlay. Before save, undo/redo,
search, sheet-sensitive actions, or navigation, the overlay is flushed through
the revision-checked mutation protocol.

```text
dirty = backend content hash differs from saved hash
        OR the document is new/recovered and has no confirmed save
        OR pending edit overlay is non-empty
```

Every save stages bytes through the backend before crossing a platform port.
Desktop writes atomically and commits the saved hash only after the write
succeeds. Web local save writes IndexedDB before committing the saved hash.
Mobile exposes the WebView handoff as an exported copy and does not commit the
saved hash because the WebView cannot confirm a durable platform write.

## Dioxus Alignment

Platform dependencies are optional and enabled only by their target feature.
The application uses the engine crate without default features for SSR and Web
UI builds, so those targets compile only the shared protocol. Desktop, mobile,
and Worker targets enable the engine's full `engine` feature.
The `web` feature enables `dioxus-web/hydrate` directly; it does not enable the
fullstack server feature. The production build first asks `dx` to compile and
post-process that hydration client, then embeds its complete public output while
compiling the Axum SSR executable with `embedded-server`. The intermediate
`web`, `server`, and `worker` features are build boundaries, not alternative
deployment modes.

SSR initial state is deterministic on server and client. Browser-only state,
update checks, Worker startup, and IndexedDB reads begin after hydration. Assets
use Dioxus `asset!`; Worker glue is generated independently by wasm-bindgen and
then embedded with the Dioxus client output.

WebAssembly requires JavaScript binding glue to enter browser APIs. Dioxus and
wasm-bindgen generate that glue under `target/`; it is a delivery artifact, not
application source. Platform bridges that call WebView/browser APIs remain in
Rust through Dioxus, `web-sys`, and `wasm-bindgen`.

Official references:

- [Dioxus platform features](https://dioxuslabs.com/learn/0.7/guides/platforms/)
- [Dioxus fullstack project setup](https://dioxuslabs.com/learn/0.7/essentials/fullstack/project_setup/)
- [Dioxus SSR and hydration](https://dioxuslabs.com/learn/0.7/essentials/fullstack/ssr/)
- [Dioxus assets](https://dioxuslabs.com/learn/0.7/essentials/ui/assets/)
- [Dioxus mobile guide](https://dioxuslabs.com/learn/0.7/guides/platforms/mobile/)
- [Dioxus Components](https://github.com/DioxusLabs/dioxus-components)

## Known Constraints

- iOS and Android compile checks cover shared Rust and Dioxus code. File picker,
  download, external-link, and close-guard flows still require device-level
  acceptance tests.
- Dioxus Desktop 0.7.10 reaches `block` 0.1.6 through its macOS WebView stack.
  Rust reports that upstream crate as future-incompatible; replacing it locally
  would diverge from the supported Dioxus desktop dependency graph.

## Verification Matrix

`cargo xtask check` covers desktop, SSR, Web Wasm, and the Web Worker Wasm
targets. Strict Clippy denies Rust warnings and redundant, copy, or implicit
clones. Mobile is checked separately with the platform commands in `AGENTS.md`.
