# AGENTS.md

Guidance for AI coding agents working in this repository.

## Project Overview

Simple Table is a cross-platform spreadsheet editor for Excel/CSV-style files, built with Vue 3, TypeScript, Element Plus, Rust, and Tauri 2.

The app targets desktop and mobile Tauri builds. File operations are abstracted by platform-specific frontend modules and Rust commands.

## Common Commands

Use the project-local Node/npm when available:

```bash
/Library/NodeJs/graalnode24-25.1.3-macos-aarch64/bin/npm run build
```

Standard commands:

```bash
npm install
npm run dev
npm run build
npm run preview
npm run tauri -- dev
npm run tauri -- build
```

Rust checks:

```bash
cd src-tauri
cargo fmt
cargo check
cargo test
```

## Architecture

Frontend source lives in `src/`:

- `components/` - Vue components for table editing, cells, toolbar, search, updates, and layout.
- `views/` - Page-level views such as `HomeView` and `TableView`.
- `composables/` - Shared Vue logic such as file actions, platform detection, pending cell saves, document status, and updates.
- `stores/` - Pinia stores for loaded file data and recent files.
- `platform/` - Platform-specific frontend file APIs for desktop, Android, and iOS.
- `types/` - TypeScript data contracts shared by views and components.
- `router/` - Vue Router setup.
- `styles/` - Base and platform CSS.

Rust backend source lives in `src-tauri/src/`:

- `commands/` - Tauri command entry points.
- `ops/` - Editor operations, undo/redo, search, and indexing.
- `io/` - File readers/writers and platform I/O.
- `state/` - Global editor state, content hash dirty tracking, and state info.
- `types/` - Rust data structures serialized to/from the frontend.
- `recent/` - Recent file storage and thumbnail handling.
- `update/` - Mobile update checks.
- `error/` - `AppError` and error conversions.

Root module files such as `ops.rs`, `state.rs`, and `types.rs` declare submodules and re-export public APIs.

## Dirty State

Unsaved-change display is driven by Rust-side content hashing plus a frontend pending-edit overlay.

- Rust hashes saved file content: sheet names, rows, merge ranges, and persisted row/column dimensions.
- Runtime-only fields such as path, file name, and search indexes do not count as dirty content.
- `EditorStateInfo.isDirty` reports whether current content hash differs from the last saved hash.
- Frontend `useDocumentStatus()` combines backend dirty state with pending debounce edits:

```ts
hasUnsavedChanges = isContentDirty || hasPendingContentChange
```

Column and row resizing are persisted document edits and should participate in `Unsaved changes`, undo, and redo.

## File And Save Flow

- Open/new file initializes Rust `EditorState`.
- Content operations go through Rust operations where possible.
- Cell editing uses a frontend debounce layer; pending edits must be flushed before save, undo/redo, search, sheet changes that depend on committed data, or navigation.
- `saveFile(...)` commits the prepared bytes through the backend save protocol, which updates the Rust saved hash after the on-disk write succeeds.
- Do not manually set unsaved UI state in random components. Refresh editor state via the document status composable.

## Code Guidelines

### TypeScript/Vue

- Use `import type` for type-only imports.
- Prefer existing composables and platform abstractions over direct Tauri invokes in components.
- Keep file operations in `src/composables/useFileActions.ts` or `src/platform/`.
- Keep status/dirty handling in `src/composables/useDocumentStatus.ts`.
- Avoid inline styles in Vue components; use scoped CSS.

### Rust

- Use `AppError` for fallible command and operation results.
- Keep Tauri command wrappers thin; place business logic in `ops/`, `io/`, `state/`, or related modules.
- Run `cargo fmt` after Rust edits.
- When adding commands, register them in `src-tauri/src/lib.rs` and expose frontend wrappers in `src/api.ts`.

## Versioning And Releases

Version locations that should stay aligned:

- `package.json`
- `package-lock.json`
- `src-tauri/Cargo.toml`
- `src-tauri/Cargo.lock` package entry for `simple-table`
- `src-tauri/tauri.conf.json`

Tags use semantic versioning with a `v` prefix, such as `v0.11.0`.

## Verification Expectations

Before finishing non-trivial code changes, run:

```bash
cd src-tauri && cargo check
/Library/NodeJs/graalnode24-25.1.3-macos-aarch64/bin/npm run build
```

For backend state/hash changes, also run:

```bash
cd src-tauri && cargo test
```

The frontend build may emit Rolldown warnings from dependencies or chunk-size warnings; those are not failures if the build exits successfully.
