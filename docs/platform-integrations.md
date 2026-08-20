# Platform Integration Provenance

This document records whether a cross-platform capability is provided directly
by an upstream project or implemented in Simple Table. It is the source of truth
for claims that a feature is "official."

## Classification

- **Official - Dioxus**: the application uses a public Dioxus or Dioxus
  Components API without replacing its implementation.
- **Official - platform**: Android, Apple, or browser documentation defines the
  underlying API. Any Rust port, JNI call, orchestration, and error handling in
  this repository are still **project code**.
- **Upstream library**: an unmodified third-party dependency provides part of
  the implementation, but it is not an official Dioxus or operating-system API.
- **Project**: the behavior and implementation are owned by Simple Table.
- **Compatibility**: project code works around a confirmed upstream gap. It is
  not an upstream patch and has explicit removal conditions.

Using an official operating-system API does not make the complete feature an
upstream feature. For example, Android `MediaStore` is official, while
`ports/android.rs`, its JNI calls, save-token handling, and UI feedback are
implemented by this project.

## Capability Matrix

| Capability | Classification | Ownership and implementation |
| --- | --- | --- |
| Desktop, mobile, SSR, hydration, routing, events, and `asset!` | **Official - Dioxus** | Public Dioxus 0.7.10 APIs and renderers are used directly. Target composition and application behavior are **Project**. |
| Switch, Tabs, and Toolbar primitives | **Official - Dioxus** | Imported directly from the official `dioxus-primitives` repository at the audited commit in `Cargo.toml`. App styling, labels, enabled state, and commands are **Project**. |
| Toolbar icons | **Upstream library + Project** | Icon data comes from `lucide-icons`; the Dioxus adapter, sizing, accessibility labels, and button behavior are **Project**. |
| Desktop and Web workbook/image selection | **Official - Dioxus** | RSX file inputs use Dioxus `onchange`, `FormData`, and file-reading APIs. Validation and engine requests are **Project**. |
| Desktop save/export dialog | **Upstream library + Project** | `rfd::AsyncFileDialog` supplies the native dialog. Path policy, atomic writes, save tokens, and document identity are **Project**. |
| Web download | **Official - browser + Project** | Browser Blob/object-URL APIs are called through `web-sys`; the Rust port and export workflow are **Project**. |
| Web recovery and local save | **Project** | The Rust Web Worker, protocol, IndexedDB persistence, recovery policy, and stateless SSR boundary are implemented by Simple Table. |
| Android save/export | **Official - platform + Project** | The project JNI adapter calls Android `MediaStore.Downloads` and `ContentResolver`. It writes to `Download/Simple Table` and retains the returned content URI for later saves. |
| iOS save/export | **Official - platform + Project** | Project code atomically writes the app Documents directory. Official `UIFileSharingEnabled` and `LSSupportsOpeningDocumentsInPlace` plist keys expose those documents to Files and permit in-place access. |
| Android/iOS workbook and image selection | **Compatibility** | A project-owned Rust port uses official Dioxus `document::eval` to create a standard WebView file input at runtime, then reads the selected bytes. The reason and removal rules are below. |
| Mobile recovery snapshots | **Project** | Recovery uses app-private storage and the engine's atomic writer. Android obtains `getFilesDir` through JNI; iOS resolves its local data directory. |
| Image double-click preview | **Official event + Project interaction** | Dioxus `ondoubleclick` delivers the event. The preview modal, keyboard behavior, selected-image state, and touch toolbar action are **Project**. `touch-action: manipulation` is a standard Pointer Events declaration. |
| Grid scrolling and virtualization | **Project** | Region caching, geometry, scroll synchronization, flicker mitigation, row/column sizing, and responsive behavior are implemented by Simple Table. |
| Workbook engine | **Project** | The protocol, document model, operations, formula/search adapters, dirty state, undo/redo, revision checks, and import/export orchestration are project behavior, even where third-party Rust libraries are used internally. |

## Code Locations

- Dependency versions and pins: [`Cargo.toml`](../Cargo.toml)
- File selection and per-target writes:
  [`apps/simple-table/src/ports/file.rs`](../apps/simple-table/src/ports/file.rs)
- Android `MediaStore` JNI adapter:
  [`apps/simple-table/src/ports/android.rs`](../apps/simple-table/src/ports/android.rs)
- Save preparation and commit workflow:
  [`apps/simple-table/src/actions.rs`](../apps/simple-table/src/actions.rs)
- Mobile recovery:
  [`apps/simple-table/src/ports/recovery.rs`](../apps/simple-table/src/ports/recovery.rs)
- Image interaction and grid behavior:
  [`apps/simple-table/src/components/grid.rs`](../apps/simple-table/src/components/grid.rs)
- Platform plist and SDK settings: [`Dioxus.toml`](../Dioxus.toml)

## Mobile File Selection Compatibility

Dioxus 0.7.10 does not provide a working native file-selection result on
Android or iOS. Its desktop file-upload implementation explicitly returns an
empty list for non-desktop targets, and the native interpreter intercepts
Dioxus-managed file inputs before asking that host implementation for files.
The upstream mobile file-dialog request is tracked as an enhancement rather
than a completed mobile API.

Simple Table therefore creates a Dioxus-unmanaged HTML file input through the
official `document::eval` API. This delegates selection to the Android/iOS
WebView's standard file-input flow. A `FileReader` returns the bytes to Rust,
where the same validation and engine operations used by other targets continue
normally.

This is **Compatibility**, not a Dioxus patch:

- no Dioxus source is modified or vendored;
- no Cargo `[patch]` overrides Dioxus;
- no JavaScript or TypeScript source is checked in; the short WebView program
  remains inside the Rust platform port;
- Dioxus 0.7.10 remains the application dependency.

Remove this compatibility path only after all of the following are true:

1. An adopted Dioxus release returns real file data from its supported Android
   and iOS file-selection API.
2. Its native interpreter no longer redirects the application back to an empty
   host result.
3. Device tests pass for workbook selection, image selection, and cancellation
   on both Android and iOS using that official API.

Until then, replacing the compatibility port with an ordinary Dioxus-managed
RSX file input would knowingly restore the empty-result behavior on mobile.

## Mobile Storage Policy

Android targets API 29 or newer. Files created by this application through
`MediaStore.Downloads` do not require broad storage permission. Existing
content URIs are updated through `ContentResolver`; a new export creates a new
Downloads item.

iOS saves to the app Documents directory and exposes it through the official
document-sharing plist keys. Saving updates an existing app-owned path after a
successful write; exporting creates a separate name. On both platforms, the
backend commits the saved hash only after the platform write succeeds.

These integrations use official platform facilities, but their implementation
and product semantics remain **Project** code.

## Dependency Policy

The repository does not carry local Dioxus patches. The exact Dioxus release is
pinned in the workspace. `dioxus-primitives` is an official upstream Git
dependency pinned to an audited revision because no suitable crates.io release
is available; a Git revision pin is not a source patch.

Before a mobile release, compile checks must be supplemented with device tests
for open/cancel, insert image, first save, repeated save, export copy, recovery,
external links, and the unsaved-change guard on both Android and iOS.

## Official References

- [Dioxus 0.7.10 mobile file-upload branch](https://github.com/DioxusLabs/dioxus/blob/v0.7.10/packages/desktop/src/file_upload.rs#L38-L61)
- [Dioxus 0.7.10 native file-input interception](https://github.com/DioxusLabs/dioxus/blob/v0.7.10/packages/interpreter/src/ts/native.ts#L67-L138)
- [Dioxus mobile file-dialog enhancement](https://github.com/DioxusLabs/dioxus/issues/3855)
- [Dioxus document evaluation](https://dioxuslabs.com/learn/0.7/essentials/ui/escape/)
- [Dioxus event and file-input handling](https://dioxuslabs.com/learn/0.7/essentials/basics/event_handlers/)
- [Dioxus mobile guide](https://dioxuslabs.com/learn/0.7/guides/platforms/mobile/)
- [Dioxus Components](https://github.com/DioxusLabs/dioxus-components)
- [Android shared-media storage permissions](https://developer.android.com/training/data-storage/shared/media#storage-permission)
- [Android `MediaStore.Downloads`](https://developer.android.com/reference/android/provider/MediaStore.Downloads)
- [Apple launch-services plist keys](https://developer.apple.com/library/archive/documentation/General/Reference/InfoPlistKeyReference/Articles/LaunchServicesKeys.html)
- [Apple iOS plist keys](https://developer.apple.com/library/archive/documentation/General/Reference/InfoPlistKeyReference/Articles/iPhoneOSKeys.html)
- [Pointer Events `touch-action`](https://www.w3.org/TR/pointerevents/#the-touch-action-css-property)
