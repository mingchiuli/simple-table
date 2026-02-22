# Simple Table

> ⚠️ **Vibe Coding Project** - This is a vibe coding project. Code quality may vary.

A cross-platform desktop application for viewing and editing Excel/CSV files, built with Tauri 2.0 and Vue 3.

## Features

- Open and edit Excel files (.xlsx, .xls, .ods)
- Open and edit CSV files
- Multi-sheet support
- Add/delete rows and columns
- Save changes to file

## Installation

### From Release

Download the latest release from the [Releases](https://github.com/mingchiuli/simple-table/releases) page.

#### macOS Installation Note

If you see "The file is damaged and cannot be opened" error on macOS after installation, run the following command in terminal:

```bash
sudo xattr -rd com.apple.quarantine "/Applications/simple-table.app"
```

This is required because the app is not signed/notarized. For a permanent solution, you'll need an Apple Developer account to sign and notarize the app.

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

## License

MIT
