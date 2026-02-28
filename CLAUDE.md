# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Simple Table is a cross-platform desktop application for viewing and editing Excel/CSV files, built with Tauri 2.0 and Vue 3.

## Tech Stack

- **Frontend**: Vue 3 + TypeScript + Element Plus
- **Backend**: Rust + Tauri 2.0
- **Excel Processing**: calamine (read) + xlsxwriter (write)

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
- `stores/` - Pinia state management
- `types/` - TypeScript type definitions
- `router/` - Vue Router configuration

### Backend Structure (`src-tauri/`)
- Rust backend for file I/O and Excel processing using calamine and xlsxwriter

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
