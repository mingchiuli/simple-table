# Simple Table

> A cross-platform desktop application for viewing and editing Excel/CSV files, built with Tauri 2.0 and Vue 3.

## Features

### Core Features
- Open and edit Excel files (.xlsx, .xls, .ods)
- Open and edit CSV files
- Multi-sheet support (add/delete sheets)
- Add/delete rows and columns
- Save changes to file
- Search functionality across sheets
- Undo/Redo support
- Preserve merged cells
- Column resizing with persistence
- Large integer precision preservation (up to 2^53)

### Platform Support
- **Desktop**: macOS, Linux, Windows
- **Mobile**: Android (APK builds available via GitHub Releases)

### Limitations

- **Does not support Excel styles**: Font colors, background colors, borders, cell alignment, and other formatting styles are not preserved. Only cell values and merged cell information are maintained.

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
- **Excel Processing**: calamine (read) + xlsxwriter (write)
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
└── styles/               # Platform-specific styles

src-tauri/                # Rust backend
├── src/
│   ├── commands/         # Tauri commands
│   ├── ops/              # Operations (cell, sheet, sort, search)
│   ├── io/               # File I/O (reader, writer)
│   ├── types/            # Rust types
│   └── state/            # Editor state management
```

## License

MIT
