# Simple Table Components

This crate is the only UI-library boundary for the application.

- `src/components/` and `assets/dx-components-theme.css` are generated from the
  official Dioxus Components registry. Do not edit them locally.
- `src/lib.rs` is the project facade. It exports the generated components,
  official Lucide icons, primitive API types required by component props, and
  the theme asset.
- Application-specific composition belongs in `apps/simple-table/src/ui.rs`.
- Application-specific colors and layout belong in
  `apps/simple-table/assets/main.css`.

Refresh the generated source at the audited revision with:

```bash
cargo xtask components
cargo fmt --all -- --check
cargo xtask check
cargo xtask test
```

To upgrade Dioxus Components, update `DIOXUS_COMPONENTS_REVISION` in
`xtask/src/main.rs` and the matching `dioxus-primitives` revision in this
crate's `Cargo.toml`, then run the refresh command and review the generated
diff. The application crate must continue to depend only on this facade.
