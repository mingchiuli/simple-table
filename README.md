# Simple Table

> A cross-platform spreadsheet editor for Excel and CSV files, built with Tauri 2 and Vue 3.

## Features

### Core Features
- Open and edit Excel files (.xlsx)
- Open and edit CSV files
- Multi-sheet support (add/delete sheets)
- Add/delete rows and columns
- Save changes to file
- Search functionality across sheets
- Undo/Redo support
- Preserve merged cells
- Column resizing with persistence
- Exact unsigned 64-bit document and revision identifiers across the IPC boundary

### Platform Support
- **Desktop**: macOS, Linux, Windows
- **Mobile**: Android and iOS Tauri builds
- Platform detection via `src/composables/usePlatform.ts` and `src/utils/platform.ts`

### Limitations

- Excel styles, hyperlinks, freeze panes, and drawings are projected read-only. The original XLSX workbook remains the persistence source, so supported edits preserve this metadata, but the app does not edit rich formatting directly.
- CSV files contain cell values only and cannot persist XLSX-specific layout or rich metadata.

## Installation

### From Release

Download the latest release from the [Releases](https://github.com/mingchiuli/simple-table/releases) page.

#### macOS Installation Note

If you see "The file is damaged and cannot be opened" error on macOS after installation, run the following command in terminal:

```bash
sudo xattr -rd com.apple.quarantine "/Applications/Simple Table.app"
```

### From Source

```bash
# Install dependencies
npm install

# Development
npm run tauri dev

# Build
npm run tauri build
```

## Tech Stack

- **Frontend**: Vue 3 + TypeScript + Element Plus
- **Backend**: Rust + Tauri 2.0
- **Excel Processing**: umya-spreadsheet backed workbook patching
- **State Management**: Pinia
- **Routing**: Vue Router

## Project Structure

```
src/                      # Frontend source
├── components/           # Vue components
├── views/                # Page components
├── stores/               # Pinia stores
├── types/                # TypeScript types
├── router/               # Vue Router config
├── composables/          # Vue composables
├── platform/             # Platform-specific file operations
│   ├── desktop/         # Desktop (macOS/Linux/Windows)
│   ├── android/         # Android
│   └── ios/             # iOS
└── styles/              # Platform-specific styles

src-tauri/                # Rust backend
├── src/
│   ├── commands/         # Tauri commands
│   ├── ops/              # Operations (cell, sheet, search, undo/redo, indexing)
│   ├── io/               # File I/O, codecs, platform adapters, workbook patching
│   ├── recent/           # Recent files management (types, store, thumbnail, ops)
│   ├── utils.rs          # Utility functions
│   ├── types/            # Rust types
│   ├── state/            # Editor state management
│   └── error/            # Error handling (AppError)
│   └── lib.rs            # App setup and command registration
```

The document ownership model, mutation protocol, save transaction, and resource
boundaries are documented in [docs/architecture.md](docs/architecture.md).

## Platform Architecture

File operations are abstracted through `src/platform/` which provides a unified API:

```ts
import {
  getPlatformAPI,
  pickOpenFile,
  prepareOpenFile,
  saveFile,
  pickSaveLocation,
} from '@/platform';
```

Platform modules are **dynamically loaded** at runtime based on the current OS. Vite code-splits each platform into separate chunks for optimal bundle size.

**Important**: When adding a new platform, implement all methods in `PlatformFileOps` interface in `src/platform/types.ts`.

## License

MIT
