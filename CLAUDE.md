# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Simple Table is a cross-platform desktop application for viewing and editing Excel/CSV files, built with Tauri 2.0 and Vue 3.

## Tech Stack

- **Frontend**: Vue 3 + TypeScript + Element Plus
- **Backend**: Rust + Tauri 2.0
- **Excel Processing**: umya-spreadsheet backed workbook patching

## Common Commands

```bash
# Install dependencies
npm install

# Development
npm run tauri dev

# Build for production
npm run tauri build

# Frontend only
npm run dev      # Vite dev server
npm run build    # TypeScript check + Vite build
npm run preview  # Preview production build
```

## Architecture

### Frontend Structure (`src/`)
- `components/` - Vue components (TableEditor, EditableCell, SearchPanel, etc.)
- `views/` - Page-level components (TableView)
- `stores/` - Pinia state management (fileData, recentFiles)
- `types/` - TypeScript type definitions
- `router/` - Vue Router configuration
- `composables/` - Vue composables for platform, file actions, document status, pending edits, and updates
- `platform/` - Platform-specific file operations (desktop, android, ios)
- `styles/` - Global styles (base.css, platform.css)

### Backend Structure (`src-tauri/src/`)
- `commands/` - Tauri command handlers (common.rs, android.rs, ios.rs)
- `ops/` - Business logic operations (cell_ops, editor_ops, index_ops, search_ops)
- `io/` - File I/O, codecs, platform adapters, and workbook patching
- `recent/` - Recent files management (types.rs, store.rs, thumbnail.rs, ops.rs)
- `utils.rs` - Utility functions
- `types/` - Core data types (CellValue, FileData, SheetData, etc.)
- `state/` - Editor state management (editor_state.rs)
- `error/` - Error handling (AppError enum)

### Module Organization Pattern
- Root-level module files (`ops.rs`, `io.rs`, `recent.rs`, `utils.rs`) declare submodules and re-export public APIs
- Example: `src/ops.rs` contains `pub mod cell_ops; pub use cell_ops::*;`
- This pattern provides clean public APIs while maintaining internal organization

## Compilation Check

**Priority**: Use IDE MCP interface for compilation checks first, fallback to actual compilation only when MCP is unavailable.

### IDE MCP (Recommended)
```bash
# Use mcp__ide__getDiagnostics to check for compilation errors
mcp__ide__getDiagnostics({ uri: "file:///path/to/file.rs" })
```
- Returns `[]` if no errors, otherwise returns diagnostic messages
- Faster and provides real-time feedback from the IDE's language server

### Fallback: Cargo
```bash
# Only use when IDE MCP is unavailable
cargo check
cargo build
```

## Commit Standards

### Commit Message Format
- Use English only
- Start with lowercase (e.g., "fix:", "feat:", "chore:")
- Keep subject line under 72 characters
- Include body explaining "why" not "what"
- Add Co-Authored-By footer

Example:
```
fix: auto focus on manual cell click

- Add autoFocus prop to EditableCell to control focus behavior
- Distinguish manual click vs external trigger via editingCell sync check

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>
```

### Tag Versioning
- Follow semantic versioning: v0.0.0 (major.minor.patch)
- Create tag after commit: `git tag -a v0.3.6 -m "v0.3.6"`
- Push both code and tag: `git push origin main && git push origin v0.3.6`

### Push Command Format
```
git push origin main && git push origin v0.3.6
```

## Code Guidelines

### Rust Error Handling
- Use `AppError` enum from `src/error/error.rs` for all error types
- Prefer specific error variants (`NoFileLoaded`, `NothingToUndo`, `RowNotFound`) over generic `Internal`
- Return `Result<T, AppError>` for all fallible operations

### TypeScript/Vue
- Use `import type` for type-only imports
- Organize imports: external first, then internal (`@/` aliases)
- Use Element Plus theme variables and global styles from `src/styles/` for consistent colors
- Avoid inline styles in Vue components; use scoped CSS instead
