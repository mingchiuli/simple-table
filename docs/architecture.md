# Architecture

Simple Table is a virtual Rust workspace. The Dioxus application, Web Worker,
and Web server are independent packages under `apps/`; the serializable
protocol and spreadsheet engine live under `crates/`. Platform features select
the renderer and delivery adapters without forking the editor model. The SSR
server owns no workbook session or bytes.

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

`crates/simple-table-protocol` owns the typed editor requests, replies,
projections, capabilities, and shared error DTO used by every platform.
`crates/simple-table-web-protocol` separately owns Worker envelopes and
IndexedDB workspace commands. Workbook and image bytes are attachments on the
editor command boundary; they are never encoded into the JSON metadata.

### Engine

`crates/simple-table-engine/src/lib.rs` declares `simple-table-engine`; its
implementation files retain their established responsibility boundaries:

- `document/`: workbook model, backing data, patches, restore, and save
- `ops/`: mutation execution, projections, and operation impact
- `state/`: editor session, history, search data, and dirty tracking
- `io/`: codecs, projections, limits, and atomic desktop file primitives
- `application/`: use-case services and ports
- `adapters/`: codec and search implementations

`CoreFacade::execute` accepts an editor command plus an optional binary
attachment and returns a typed reply plus an optional attachment. Platform UI
code does not reach into internal backend state.

### Application And Ports

`apps/simple-table/src/lib.rs` composes the Dioxus app. Its `components.rs` and
`components/` own the home/editor views, responsive layout, and pending-edit
coordination. `ports.rs` and `ports/` isolate native, mobile, and browser
behavior.

Views express edits as context-free mutation intents. The action coordinator
flushes pending cell edits, acquires the serialized operation lock, and only
then binds the current document ID and revision to an editor request. Controls
consume engine-projected workbook capabilities; unavailable operations stay
disabled and expose the first blocking reason.

`crates/simple-table-components` isolates the official styled Dioxus Components
source and official Dioxus Lucide icons from application code. Its generated
component tree retains the upstream layout and is refreshed with
`cargo xtask components` at the audited revision. `src/lib.rs` is the stable
project facade, `apps/simple-table/src/ui.rs` contains thin compositions, and
app-specific presentation lives in `apps/simple-table/assets/main.css`. This
keeps upstream regeneration independent from business views and theme changes.

### Browser Worker

`apps/simple-table-web-worker` owns a browser-side `CoreFacade` and persists
saved and recovery snapshots in IndexedDB. One shared Worker client correlates
editor and workspace requests by message ID. JSON envelopes carry typed
metadata from `simple-table-web-protocol`; transferable `ArrayBuffer` values
carry workbook and image bytes without JSON array expansion. IndexedDB records
are schema-versioned, and version 1 records are read and lazily rewritten
without discarding saved or recovery bytes. The Worker build is generated into
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
Android writes through `MediaStore.Downloads`; iOS writes atomically to the app
Documents directory. Both mobile targets commit the saved hash only after the
platform write succeeds and retain a durable target for subsequent saves.
Exporting a copy uses a separate read-only protocol operation, so choosing a
different output format never changes the active document identity, revision,
saved hash, or undo history.

Sorting and filtering follow the engine-owned rules documented in
[Sort And Filter Semantics](sort-and-filter.md). Sorts are document edits;
filters are session state on the same undo/redo timeline.

## Dioxus Alignment

Platform dependencies are optional and enabled only by their target feature.
SSR and Web UI builds depend directly on `simple-table-protocol`; desktop,
mobile, and Worker packages depend on the complete `simple-table-engine`.
The `web` feature enables `dioxus-web/hydrate` directly; it does not enable the
fullstack server feature. The UI facade mounts its hidden overlay asset provider
after hydration so upstream component hooks do not add server-only entries to
the initial hydration stream. The production build first asks `dx` to compile
and post-process that hydration client, then embeds its complete public output
while compiling the Axum SSR executable with the Web server's `embedded`
feature. The application package keeps only the mutually exclusive `desktop`,
`mobile`, `web`, and `server` build boundaries; Worker and Web server behavior
belongs to their own packages.

SSR initial state is deterministic on server and client. Browser-only state,
update checks, Worker startup, and IndexedDB reads begin after hydration. Assets
use Dioxus `asset!`; Worker glue is generated independently by wasm-bindgen and
then embedded with the Dioxus client output.

WebAssembly requires JavaScript binding glue to enter browser APIs. Dioxus and
wasm-bindgen generate that glue under `target/`; it is a delivery artifact, not
application source. Platform bridges that call WebView/browser APIs remain in
Rust through Dioxus, `web-sys`, and `wasm-bindgen`.

The provenance of each integration is recorded in
[Platform Integration Provenance](platform-integrations.md). It distinguishes
direct upstream features, project adapters over official platform APIs, and the
temporary mobile file-selection compatibility path.

Official references:

- [Dioxus platform features](https://dioxuslabs.com/learn/0.7/guides/platforms/)
- [Dioxus fullstack project setup](https://dioxuslabs.com/learn/0.7/essentials/fullstack/project_setup/)
- [Dioxus SSR and hydration](https://dioxuslabs.com/learn/0.7/essentials/fullstack/ssr/)
- [Dioxus assets](https://dioxuslabs.com/learn/0.7/essentials/ui/assets/)
- [Dioxus mobile guide](https://dioxuslabs.com/learn/0.7/guides/platforms/mobile/)
- [Dioxus Components](https://github.com/DioxusLabs/dioxus-components)

## Known Constraints

- iOS and Android compile checks cover shared Rust and Dioxus code but do not
  replace device acceptance tests. Open/cancel, image selection, first and
  repeated saves, export, recovery, external-link, and close-guard flows must be
  verified on both platforms before release.
- Dioxus Desktop 0.7.10 reaches `block` 0.1.6 through its macOS WebView stack.
  Rust reports that upstream crate as future-incompatible; replacing it locally
  would diverge from the supported Dioxus desktop dependency graph.

## Verification Matrix

`cargo xtask check` covers protocol, engine, desktop, SSR, Web Wasm, and the Web
Worker Wasm targets. `cargo xtask test` runs native protocol, engine, desktop,
and SSR tests. `cargo xtask test-web` runs the Web protocol and Worker tests and
checks the Worker test target for Wasm. Both test commands are required in CI.
Strict Clippy denies Rust warnings and redundant, copy, or implicit clones.
Mobile is checked separately with the platform commands in `AGENTS.md`.
