use std::rc::Rc;
use std::time::Duration;

use crate::protocol::{CellEdit, EditorReply, EditorRequest};
use base64::Engine;
use dioxus::prelude::{ReadableExt, WritableExt, spawn};
use dioxus_sdk_time::sleep;

use crate::model::{
    AppPorts, EditorMutationView, EditorPatchView, EditorStore, GridScrollRequest,
    OpenDocumentView, SavedDocumentView, SearchView, SheetViewport, request_id,
};

pub async fn new_document(store: EditorStore, ports: Rc<AppPorts>) -> bool {
    let _operation = ports.operations.lock().await;
    set_busy(store, "Creating workbook");
    match ports
        .editor
        .execute(EditorRequest::NewDocument {
            request_id: request_id("new"),
        })
        .await
    {
        Ok(reply) => {
            let opened = accept_document_reply(store, reply);
            if opened {
                refresh_images(store, Rc::clone(&ports)).await;
                schedule_recovery(store, Rc::clone(&ports));
            }
            opened
        }
        Err(error) => {
            store.set_error(error);
            false
        }
    }
}

pub async fn open_bytes(
    store: EditorStore,
    ports: Rc<AppPorts>,
    file_name: String,
    bytes: Vec<u8>,
) -> bool {
    let _operation = ports.operations.lock().await;
    set_busy(store, "Reading workbook");
    match ports
        .editor
        .execute(EditorRequest::OpenDocument {
            request_id: request_id("open"),
            file_name,
            bytes,
        })
        .await
    {
        Ok(reply) => {
            let opened = accept_document_reply(store, reply);
            if opened {
                refresh_images(store, Rc::clone(&ports)).await;
                schedule_recovery(store, Rc::clone(&ports));
            }
            opened
        }
        Err(error) => {
            store.set_error(error);
            false
        }
    }
}

pub async fn open_local(store: EditorStore, ports: Rc<AppPorts>, document_key: String) -> bool {
    let _operation = ports.operations.lock().await;
    set_busy(store, "Opening local workbook");
    match ports
        .editor
        .execute(EditorRequest::OpenLocalDocument {
            request_id: request_id("open-local"),
            document_key,
        })
        .await
    {
        Ok(reply) => {
            let opened = accept_document_reply(store, reply);
            if opened {
                refresh_images(store, Rc::clone(&ports)).await;
                schedule_recovery(store, Rc::clone(&ports));
            }
            opened
        }
        Err(error) => {
            store.set_error(error);
            false
        }
    }
}

pub async fn load_local_documents(store: EditorStore, ports: Rc<AppPorts>) {
    #[cfg(feature = "web")]
    match ports
        .editor
        .execute(EditorRequest::ListLocalDocuments)
        .await
    {
        Ok(EditorReply::LocalDocuments { documents }) => {
            let mut store = store;
            store.local_documents.set(documents);
        }
        Err(error) if error.code != "client_not_hydrated" => store.set_error(error),
        _ => {}
    }

    #[cfg(not(feature = "web"))]
    let _ = (store, ports);
}

pub async fn delete_local_document(store: EditorStore, ports: Rc<AppPorts>, document_key: String) {
    let _operation = ports.operations.lock().await;
    set_busy(store, "Removing local workbook");
    match ports
        .editor
        .execute(EditorRequest::DeleteLocalDocument {
            document_key: document_key.clone(),
        })
        .await
    {
        Ok(EditorReply::Empty) => {
            let mut store = store;
            store
                .local_documents
                .write()
                .retain(|document| document.id != document_key);
            store.busy.set(false);
            store.status.set("Local workbook removed".to_string());
        }
        Ok(_) => store.set_error(unexpected_reply("delete local document")),
        Err(error) => store.set_error(error),
    }
}

pub fn queue_cell_edit(
    mut store: EditorStore,
    ports: Rc<AppPorts>,
    sheet_index: usize,
    row: usize,
    col: usize,
    text: String,
) {
    let generation = store.edit_generation().wrapping_add(1);
    store.edit_generation.set(generation);
    store
        .pending_edits
        .write()
        .insert((sheet_index, row, col), (generation, text));
    spawn(async move {
        sleep(Duration::from_millis(500)).await;
        let should_commit = store
            .pending_edits
            .read()
            .get(&(sheet_index, row, col))
            .is_some_and(|(pending_generation, _)| *pending_generation == generation);
        if should_commit {
            let _ = flush_pending_edits(store, ports).await;
        }
    });
}

pub async fn flush_pending_edits(
    store: EditorStore,
    ports: Rc<AppPorts>,
) -> Result<(), crate::protocol::AppErrorDto> {
    let _operation = ports.operations.lock().await;
    flush_pending_edits_locked(store, Rc::clone(&ports)).await
}

async fn flush_pending_edits_locked(
    store: EditorStore,
    ports: Rc<AppPorts>,
) -> Result<(), crate::protocol::AppErrorDto> {
    while !store.pending_edits.read().is_empty() {
        flush_pending_batch_locked(store, Rc::clone(&ports)).await?;
    }
    Ok(())
}

async fn flush_pending_batch_locked(
    mut store: EditorStore,
    ports: Rc<AppPorts>,
) -> Result<(), crate::protocol::AppErrorDto> {
    let changes = store.pending_edits.read().clone();
    if changes.is_empty() {
        return Ok(());
    }
    let Some((document_id, base_revision)) = document_identity(store) else {
        let error = crate::protocol::AppErrorDto {
            code: "document_changed".to_string(),
            message: "the active document changed before edits were committed".to_string(),
        };
        store.set_error(error.clone());
        return Err(error);
    };
    let request_changes = changes
        .iter()
        .map(|((sheet_index, row, col), (_, text))| CellEdit {
            sheet_index: *sheet_index,
            row: *row,
            col: *col,
            text: text.clone(),
        })
        .collect();
    let result = run_mutation_locked(
        store,
        Rc::clone(&ports),
        EditorRequest::SetCells {
            request_id: request_id("cells"),
            document_id,
            base_revision,
            changes: request_changes,
        },
    )
    .await;
    if result.is_ok() {
        remove_committed_edits(&mut store.pending_edits.write(), changes);
    }
    result
}

pub async fn run_mutation(store: EditorStore, ports: Rc<AppPorts>, mut request: EditorRequest) {
    let _operation = ports.operations.lock().await;
    if flush_pending_edits_locked(store, Rc::clone(&ports))
        .await
        .is_err()
    {
        return;
    }
    if let Err(error) = rebase_mutation_request(store, &mut request) {
        store.set_error(error);
        return;
    }
    let _ = run_mutation_locked(store, Rc::clone(&ports), request).await;
}

async fn run_mutation_locked(
    mut store: EditorStore,
    ports: Rc<AppPorts>,
    request: EditorRequest,
) -> Result<(), crate::protocol::AppErrorDto> {
    store.busy.set(true);
    let select_added_sheet = matches!(&request, EditorRequest::AddSheet { .. });
    let previous_sheet_name = active_sheet_name(store);
    let result = match ports.editor.execute(request).await {
        Ok(EditorReply::Mutation { value }) => {
            match serde_json::from_value::<EditorMutationView>(value) {
                Ok(mutation) => {
                    let refresh =
                        MutationRefresh::for_patches(&mutation.patches, store.active_sheet());
                    store.accept_mutation(&mutation);
                    if refresh.document {
                        refresh_document(store, Rc::clone(&ports)).await;
                    }
                    if select_added_sheet {
                        select_last_sheet(store);
                    } else if active_sheet_name(store) != previous_sheet_name {
                        reset_current_sheet_viewport(store);
                    } else {
                        clamp_selected_cell(store);
                    }
                    store.search.set(None);
                    let viewport = *store.viewport.read();
                    refresh_region(
                        store,
                        Rc::clone(&ports),
                        viewport.row_start,
                        viewport.row_end,
                        viewport.col_start,
                        viewport.col_end,
                    )
                    .await;
                    sync_formula_text(store);
                    if refresh.images || select_added_sheet {
                        refresh_images(store, Rc::clone(&ports)).await;
                    }
                    schedule_recovery(store, ports);
                    store.busy.set(false);
                    Ok(())
                }
                Err(error) => Err(protocol_error(error)),
            }
        }
        Ok(_) => Err(unexpected_reply("mutation")),
        Err(error) => Err(error),
    };
    if let Err(error) = &result {
        store.set_error(error.clone());
    }
    result
}

pub async fn undo(store: EditorStore, ports: Rc<AppPorts>) {
    let Some((document_id, base_revision)) = document_identity(store) else {
        return;
    };
    run_mutation(
        store,
        ports,
        EditorRequest::Undo {
            request_id: request_id("undo"),
            document_id,
            base_revision,
        },
    )
    .await;
}

pub async fn redo(store: EditorStore, ports: Rc<AppPorts>) {
    let Some((document_id, base_revision)) = document_identity(store) else {
        return;
    };
    run_mutation(
        store,
        ports,
        EditorRequest::Redo {
            request_id: request_id("redo"),
            document_id,
            base_revision,
        },
    )
    .await;
}

pub async fn save_local(mut store: EditorStore, ports: Rc<AppPorts>) {
    let _operation = ports.operations.lock().await;
    if flush_pending_edits_locked(store, Rc::clone(&ports))
        .await
        .is_err()
    {
        return;
    }
    let Some((document_id, base_revision)) = document_identity(store) else {
        return;
    };
    let target_name = document_name(store);
    set_busy(store, "Saving workbook");

    #[cfg(feature = "web")]
    let result = ports
        .editor
        .execute(EditorRequest::SaveLocal {
            request_id: request_id("save-local"),
            document_id,
            base_revision,
            target_name,
        })
        .await;

    #[cfg(feature = "desktop")]
    let result = save_native(Rc::clone(&ports), document_id, base_revision, target_name).await;

    #[cfg(all(feature = "mobile", not(feature = "desktop")))]
    let result = save_mobile(Rc::clone(&ports), document_id, base_revision, target_name).await;

    #[cfg(all(
        feature = "server",
        not(any(feature = "web", feature = "desktop", feature = "mobile"))
    ))]
    let result = {
        let _ = (document_id, base_revision, target_name);
        Err(crate::protocol::AppErrorDto {
            code: "client_not_hydrated".to_string(),
            message: "save is unavailable during SSR".to_string(),
        })
    };

    match result {
        Ok(EditorReply::Saved { value }) => {
            match serde_json::from_value::<SavedDocumentView>(value) {
                Ok(saved) => accept_saved_document(store, saved),
                Err(error) => {
                    store.set_error(protocol_error(error));
                    return;
                }
            }
            let mut store = store;
            store.busy.set(false);
            store.status.set("Saved".to_string());
        }
        Ok(EditorReply::Empty) => {
            store.busy.set(false);
            store.status.set("Save cancelled".to_string());
        }
        Ok(_) => store.set_error(unexpected_reply("save")),
        Err(error) => store.set_error(error),
    }
}

#[cfg(feature = "desktop")]
async fn save_native(
    ports: Rc<AppPorts>,
    document_id: u64,
    base_revision: u64,
    target_name: String,
) -> Result<EditorReply, crate::protocol::AppErrorDto> {
    let Some(path) = ports
        .files
        .choose_document_path(target_name, crate::ports::file::DocumentDialogMode::Save)
        .await?
    else {
        return Ok(EditorReply::Empty);
    };
    let save_token = request_id("save-native");
    let prepared = ports
        .editor
        .execute(EditorRequest::PrepareSave {
            request_id: save_token.clone(),
            document_id,
            base_revision,
            target_name: path.clone(),
        })
        .await?;
    let EditorReply::SavePrepared {
        save_token,
        file_name,
        bytes,
    } = prepared
    else {
        return Err(unexpected_reply("prepare save"));
    };
    let path = path_for_prepared_name(path, &file_name);
    if let Err(error) = ports
        .files
        .write_document_to_path(path.clone(), bytes)
        .await
    {
        let _ = ports
            .editor
            .execute(EditorRequest::AbortSave { save_token })
            .await;
        return Err(error);
    }
    ports
        .editor
        .execute(EditorRequest::CommitSave { save_token, path })
        .await
}

#[cfg(all(feature = "mobile", not(feature = "desktop")))]
async fn save_mobile(
    ports: Rc<AppPorts>,
    document_id: u64,
    base_revision: u64,
    target_name: String,
) -> Result<EditorReply, crate::protocol::AppErrorDto> {
    let prepared = ports
        .editor
        .execute(EditorRequest::PrepareExport {
            document_id,
            base_revision,
            target_name,
        })
        .await?;
    let EditorReply::ExportPrepared { file_name, bytes } = prepared else {
        return Err(unexpected_reply("prepare export"));
    };
    ports.files.write_document(file_name, bytes).await?;
    Ok(EditorReply::Empty)
}

pub async fn download_copy(store: EditorStore, ports: Rc<AppPorts>) {
    let _operation = ports.operations.lock().await;
    if flush_pending_edits_locked(store, Rc::clone(&ports))
        .await
        .is_err()
    {
        return;
    }
    let Some((document_id, base_revision)) = document_identity(store) else {
        return;
    };
    let suggested_name = document_name(store);
    #[cfg(feature = "desktop")]
    let target_name = match ports
        .files
        .choose_document_path(
            suggested_name,
            crate::ports::file::DocumentDialogMode::Export,
        )
        .await
    {
        Ok(Some(path)) => path,
        Ok(None) => {
            let mut store = store;
            store.status.set("Download cancelled".to_string());
            return;
        }
        Err(error) => {
            store.set_error(error);
            return;
        }
    };
    #[cfg(not(feature = "desktop"))]
    let target_name = suggested_name;
    match ports
        .editor
        .execute(EditorRequest::PrepareExport {
            document_id,
            base_revision,
            target_name: target_name.clone(),
        })
        .await
    {
        Ok(EditorReply::ExportPrepared { file_name, bytes }) => {
            #[cfg(feature = "desktop")]
            let write = {
                let path = path_for_prepared_name(target_name, &file_name);
                ports
                    .files
                    .write_document_to_path(path.clone(), bytes)
                    .await
                    .map(|()| Some(path))
            };
            #[cfg(not(feature = "desktop"))]
            let write = ports.files.write_document(file_name, bytes).await;
            match write {
                Ok(Some(_)) => {
                    let mut store = store;
                    store.status.set("Copy downloaded".to_string());
                }
                #[cfg(feature = "mobile")]
                Ok(None) => {
                    let mut store = store;
                    store.status.set("Copy sent to device".to_string());
                }
                #[cfg(not(feature = "mobile"))]
                Ok(None) => {}
                Err(error) => store.set_error(error),
            }
        }
        Ok(_) => store.set_error(unexpected_reply("download")),
        Err(error) => store.set_error(error),
    }
}

pub async fn search(store: EditorStore, ports: Rc<AppPorts>, query: String, all_sheets: bool) {
    let _operation = ports.operations.lock().await;
    if flush_pending_edits_locked(store, Rc::clone(&ports))
        .await
        .is_err()
    {
        return;
    }
    let Some((document_id, base_revision)) = document_identity(store) else {
        return;
    };
    match ports
        .editor
        .execute(EditorRequest::Search {
            document_id,
            base_revision,
            query,
            current_sheet_index: Some(store.active_sheet()),
            all_sheets,
        })
        .await
    {
        Ok(EditorReply::Search { value }) => match serde_json::from_value::<SearchView>(value) {
            Ok(response) => {
                let mut store = store;
                store.search.set(Some(response));
                store.search_open.set(true);
            }
            Err(error) => store.set_error(protocol_error(error)),
        },
        Ok(_) => store.set_error(unexpected_reply("search")),
        Err(error) => store.set_error(error),
    }
}

pub async fn select_sheet(mut store: EditorStore, ports: Rc<AppPorts>, sheet_index: usize) {
    let _operation = ports.operations.lock().await;
    if flush_pending_edits_locked(store, Rc::clone(&ports))
        .await
        .is_err()
    {
        return;
    }
    store.active_sheet.set(sheet_index);
    store.selected_cell.set((0, 0));
    store.formula_text.set(String::new());
    store.grid_scroll_request.set(Some(GridScrollRequest {
        sheet_index,
        row: 0,
        col: 0,
    }));
    let viewport = SheetViewport::default();
    refresh_region(
        store,
        Rc::clone(&ports),
        viewport.row_start,
        viewport.row_end,
        viewport.col_start,
        viewport.col_end,
    )
    .await;
    refresh_images(store, Rc::clone(&ports)).await;
}

pub async fn select_search_result(
    mut store: EditorStore,
    ports: Rc<AppPorts>,
    sheet_index: usize,
    row: usize,
    col: usize,
) {
    let _operation = ports.operations.lock().await;
    if flush_pending_edits_locked(store, Rc::clone(&ports))
        .await
        .is_err()
    {
        return;
    }
    let row_start = row.saturating_sub(6);
    let col_start = col.saturating_sub(4);
    store.active_sheet.set(sheet_index);
    store.selected_cell.set((row, col));
    store.formula_text.set(String::new());
    store.grid_scroll_request.set(Some(GridScrollRequest {
        sheet_index,
        row: row_start,
        col: col_start,
    }));
    refresh_region(
        store,
        Rc::clone(&ports),
        row_start,
        row_start.saturating_add(36),
        col_start,
        col_start.saturating_add(16),
    )
    .await;
    if let Some(value) = store
        .display_cell_map(sheet_index)
        .get(&(row, col))
        .cloned()
    {
        store.formula_text.set(value);
    }
    refresh_images(store, Rc::clone(&ports)).await;
}

pub async fn refresh_images(mut store: EditorStore, ports: Rc<AppPorts>) {
    let Some((document_id, base_revision)) = document_identity(store) else {
        return;
    };
    let sheet_index = store.active_sheet();
    let items = match load_image_catalog(
        ports.editor.as_ref(),
        document_id,
        base_revision,
        sheet_index,
    )
    .await
    {
        Ok(items) => items,
        Err(error) => {
            store.set_error(error);
            return;
        }
    };

    let previous_items = store.images.read().clone();
    let previous_assets = store.image_assets.read().clone();
    let mut assets = std::collections::HashMap::new();
    for image in &items {
        if !image.renderable {
            continue;
        }
        let unchanged_asset = previous_items.iter().any(|previous| {
            previous.id == image.id
                && previous.media_id == image.media_id
                && previous.mime_type == image.mime_type
        });
        if unchanged_asset && let Some(asset) = previous_assets.get(&image.id) {
            assets.insert(image.id.clone(), Rc::clone(asset));
            continue;
        }
        match ports
            .editor
            .execute(EditorRequest::ImageBytes {
                document_id,
                base_revision,
                sheet_index,
                image_id: image.id.clone(),
            })
            .await
        {
            Ok(EditorReply::Bytes { bytes }) => {
                let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
                assets.insert(
                    image.id.clone(),
                    Rc::<str>::from(format!("data:{};base64,{encoded}", image.mime_type)),
                );
            }
            Ok(_) => {}
            Err(error) => {
                store.set_error(error);
                return;
            }
        }
    }
    if store
        .selected_image
        .read()
        .as_ref()
        .is_some_and(|id| !items.iter().any(|image| &image.id == id))
    {
        store.selected_image.set(None);
    }
    store.images.set(Rc::new(items));
    store.image_assets.set(Rc::new(assets));
}

async fn load_image_catalog(
    editor: &dyn crate::ports::editor::EditorPort,
    document_id: u64,
    base_revision: u64,
    sheet_index: usize,
) -> Result<Vec<crate::protocol::SheetImageDto>, crate::protocol::AppErrorDto> {
    let mut items = Vec::new();
    let mut offset = 0;
    loop {
        let next_offset = match editor
            .execute(EditorRequest::SheetImages {
                document_id,
                base_revision,
                sheet_index,
                offset,
                limit: 256,
            })
            .await
        {
            Ok(EditorReply::Images {
                items: page,
                next_offset,
            }) => {
                items.extend(page);
                next_offset
            }
            Ok(_) => return Err(unexpected_reply("image catalog")),
            Err(error) => return Err(error),
        };
        let Some(next_offset) = next_offset else {
            break;
        };
        if next_offset <= offset {
            return Err(crate::protocol::AppErrorDto {
                code: "protocol_error".to_string(),
                message: "image catalog returned a non-advancing page cursor".to_string(),
            });
        }
        offset = next_offset;
    }
    Ok(items)
}

pub async fn close_document(mut store: EditorStore, ports: Rc<AppPorts>) -> bool {
    let _operation = ports.operations.lock().await;
    let Some((document_id, base_revision)) = document_identity(store) else {
        return true;
    };
    match ports
        .editor
        .execute(EditorRequest::CloseDocument {
            request_id: request_id("close"),
            document_id,
            base_revision,
        })
        .await
    {
        Ok(EditorReply::Closed) => {
            store.document.set(None);
            store.region.set(None);
            store.images.set(Rc::new(Vec::new()));
            store
                .image_assets
                .set(Rc::new(std::collections::HashMap::new()));
            store.pending_edits.write().clear();
            true
        }
        Ok(_) => {
            store.set_error(unexpected_reply("close"));
            false
        }
        Err(error) => {
            store.set_error(error);
            false
        }
    }
}

pub async fn refresh_region(
    mut store: EditorStore,
    ports: Rc<AppPorts>,
    row_start: usize,
    row_end: usize,
    col_start: usize,
    col_end: usize,
) {
    let Some((document_id, base_revision)) = document_identity(store) else {
        return;
    };
    let sheet_index = store.active_sheet();
    let Some(viewport) = normalized_viewport(
        store,
        sheet_index,
        SheetViewport {
            row_start,
            row_end,
            col_start,
            col_end,
        },
    ) else {
        return;
    };
    let generation = (*store.region_generation.read()).wrapping_add(1);
    store.region_generation.set(generation);
    store.viewport.set(viewport);
    let response = ports
        .editor
        .execute(EditorRequest::Region {
            document_id,
            base_revision,
            sheet_index,
            row_start: viewport.row_start,
            row_end: viewport.row_end,
            col_start: viewport.col_start,
            col_end: viewport.col_end,
        })
        .await;
    let request_is_current = || {
        *store.region_generation.read() == generation
            && store.active_sheet() == sheet_index
            && document_identity(store) == Some((document_id, base_revision))
    };
    match response {
        Ok(EditorReply::Region { value }) => match serde_json::from_value(value) {
            Ok(region) if request_is_current() => {
                store.region.set(Some(region));
            }
            Ok(_) => {}
            Err(error) if request_is_current() => store.set_error(protocol_error(error)),
            Err(_) => {}
        },
        Ok(_) if request_is_current() => store.set_error(unexpected_reply("region")),
        Err(error) if request_is_current() => store.set_error(error),
        _ => {}
    }
}

async fn refresh_document(store: EditorStore, ports: Rc<AppPorts>) {
    match ports.editor.execute(EditorRequest::ActiveDocument).await {
        Ok(EditorReply::Document { value }) if !value.is_null() => {
            match serde_json::from_value::<OpenDocumentView>(value) {
                Ok(document) => store.refresh_document(document),
                Err(error) => store.set_error(protocol_error(error)),
            }
        }
        Ok(EditorReply::Document { .. }) => {}
        Ok(_) => store.set_error(unexpected_reply("document")),
        Err(error) => store.set_error(error),
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct MutationRefresh {
    document: bool,
    images: bool,
}

impl MutationRefresh {
    fn for_patches(patches: &[EditorPatchView], active_sheet: usize) -> Self {
        let mut refresh = Self::default();
        for patch in patches {
            match patch {
                EditorPatchView::Cells { .. } => {}
                EditorPatchView::Layout { .. } => refresh.document = true,
                EditorPatchView::ImageUpserted { patch }
                | EditorPatchView::ImageDeleted { patch }
                    if patch.sheet_index == active_sheet =>
                {
                    refresh.images = true;
                }
                EditorPatchView::ImageUpserted { .. } | EditorPatchView::ImageDeleted { .. } => {}
                EditorPatchView::RowInserted { patch } | EditorPatchView::RowDeleted { patch }
                    if patch.sheet_index == active_sheet =>
                {
                    refresh.document = true;
                    refresh.images = true;
                }
                EditorPatchView::ColumnInserted { patch }
                | EditorPatchView::ColumnDeleted { patch }
                    if patch.sheet_index == active_sheet =>
                {
                    refresh.document = true;
                    refresh.images = true;
                }
                EditorPatchView::RowInserted { .. }
                | EditorPatchView::RowDeleted { .. }
                | EditorPatchView::ColumnInserted { .. }
                | EditorPatchView::ColumnDeleted { .. } => refresh.document = true,
                EditorPatchView::SheetInvalidated { patch } => {
                    refresh.document = true;
                    refresh.images |= patch.sheet_index == active_sheet;
                }
                EditorPatchView::SheetInserted { .. }
                | EditorPatchView::SheetDeleted { .. }
                | EditorPatchView::SheetsReplaced { .. }
                | EditorPatchView::ResyncRequired { .. } => {
                    refresh.document = true;
                    refresh.images = true;
                }
            }
        }
        refresh
    }
}

fn clamp_selected_cell(mut store: EditorStore) {
    let selected = store.selected_cell();
    let clamped = store
        .document
        .read()
        .as_ref()
        .and_then(|document| document.document.sheets.get(store.active_sheet()))
        .map(|sheet| {
            (
                selected.0.min(sheet.extent.row_count.saturating_sub(1)),
                selected.1.min(sheet.extent.column_count.saturating_sub(1)),
            )
        })
        .unwrap_or((0, 0));
    if selected != clamped {
        store.selected_cell.set(clamped);
        store.grid_scroll_request.set(Some(GridScrollRequest {
            sheet_index: store.active_sheet(),
            row: clamped.0,
            col: clamped.1,
        }));
    }
}

fn sync_formula_text(mut store: EditorStore) {
    let sheet_index = store.active_sheet();
    let selected = store.selected_cell();
    let value = store
        .display_cell_map(sheet_index)
        .get(&selected)
        .cloned()
        .unwrap_or_default();
    store.formula_text.set(value);
}

fn select_last_sheet(mut store: EditorStore) {
    let last_sheet = store.document.read().as_ref().map_or(0, |document| {
        document.document.sheets.len().saturating_sub(1)
    });
    store.active_sheet.set(last_sheet);
    store.selected_cell.set((0, 0));
    store.formula_text.set(String::new());
    store.viewport.set(crate::model::SheetViewport::default());
    store.grid_scroll_request.set(Some(GridScrollRequest {
        sheet_index: last_sheet,
        row: 0,
        col: 0,
    }));
}

fn reset_current_sheet_viewport(mut store: EditorStore) {
    let sheet_index = store.active_sheet();
    store.selected_cell.set((0, 0));
    store.formula_text.set(String::new());
    store.viewport.set(SheetViewport::default());
    store.grid_scroll_request.set(Some(GridScrollRequest {
        sheet_index,
        row: 0,
        col: 0,
    }));
}

fn schedule_recovery(store: EditorStore, ports: Rc<AppPorts>) {
    #[cfg(feature = "web")]
    {
        let generation = store.edit_generation().wrapping_add(1);
        let mut store = store;
        store.edit_generation.set(generation);
        spawn(async move {
            sleep(Duration::from_secs(2)).await;
            if store.edit_generation() != generation {
                return;
            }
            let Some((document_id, base_revision)) = document_identity(store) else {
                return;
            };
            let is_dirty = store
                .document
                .read()
                .as_ref()
                .is_some_and(|document| document.editor_session.editor_state.is_dirty);
            if is_dirty {
                let _ = ports
                    .editor
                    .execute(EditorRequest::CheckpointRecovery {
                        request_id: request_id("recovery"),
                        document_id,
                        base_revision,
                        target_name: document_name(store),
                    })
                    .await;
            } else {
                let _ = ports.editor.execute(EditorRequest::ClearRecovery).await;
            }
        });
    }

    #[cfg(not(feature = "web"))]
    let _ = (store, ports);
}

fn accept_document_reply(mut store: EditorStore, reply: EditorReply) -> bool {
    let EditorReply::Document { value } = reply else {
        store.set_error(unexpected_reply("document"));
        return false;
    };
    if value.is_null() {
        store.document.set(None);
        store.region.set(None);
        store.busy.set(false);
        return false;
    }
    match serde_json::from_value::<OpenDocumentView>(value) {
        Ok(document) => {
            store.accept_document(document);
            true
        }
        Err(error) => {
            store.set_error(protocol_error(error));
            false
        }
    }
}

fn document_identity(store: EditorStore) -> Option<(u64, u64)> {
    store.document.read().as_ref().map(|document| {
        (
            document.editor_session.document_id,
            document.editor_session.revision,
        )
    })
}

fn document_name(store: EditorStore) -> String {
    store
        .document
        .read()
        .as_ref()
        .map(|document| document.document.file_name.clone())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "untitled.xlsx".to_string())
}

fn active_sheet_name(store: EditorStore) -> Option<String> {
    let document = store.document.read();
    document
        .as_ref()?
        .document
        .sheets
        .get(store.active_sheet())
        .map(|sheet| sheet.name.clone())
}

fn accept_saved_document(mut store: EditorStore, saved: SavedDocumentView) {
    if let Some(document) = store.document.write().as_mut().map(Rc::make_mut) {
        merge_saved_document(document, saved);
    }
}

fn merge_saved_document(document: &mut OpenDocumentView, saved: SavedDocumentView) {
    if let Some(manifest) = saved.document {
        document.document = manifest;
    }
    if let Some(identity) = saved.identity {
        document.document.path = identity.path;
        document.document.file_name = identity.file_name;
    }
    document.editor_session = saved.editor_session;
}

fn normalized_viewport(
    store: EditorStore,
    sheet_index: usize,
    viewport: SheetViewport,
) -> Option<SheetViewport> {
    let document = store.document.read();
    let sheet = document.as_ref()?.document.sheets.get(sheet_index)?;
    Some(clamp_viewport(viewport, sheet.extent))
}

fn clamp_viewport(viewport: SheetViewport, extent: crate::model::SheetExtentView) -> SheetViewport {
    let (row_start, row_end) =
        normalize_axis(viewport.row_start, viewport.row_end, extent.row_count);
    let (col_start, col_end) =
        normalize_axis(viewport.col_start, viewport.col_end, extent.column_count);
    SheetViewport {
        row_start,
        row_end,
        col_start,
        col_end,
    }
}

fn normalize_axis(start: usize, end: usize, extent: usize) -> (usize, usize) {
    if extent == 0 {
        return (0, 0);
    }
    let length = end.saturating_sub(start).max(1);
    let start = start.min(extent - 1);
    (start, start.saturating_add(length).min(extent))
}

#[cfg(feature = "desktop")]
fn path_for_prepared_name(selected_path: String, prepared_name: &str) -> String {
    let mut path = std::path::PathBuf::from(&selected_path);
    if path.file_name().and_then(|name| name.to_str()) != Some(prepared_name) {
        path.set_file_name(prepared_name);
    }
    path.to_string_lossy().into_owned()
}

fn rebase_mutation_request(
    store: EditorStore,
    request: &mut EditorRequest,
) -> Result<(), crate::protocol::AppErrorDto> {
    let Some((current_document_id, current_revision)) = document_identity(store) else {
        return Err(crate::protocol::AppErrorDto {
            code: "no_document".to_string(),
            message: "no workbook is open".to_string(),
        });
    };
    let context = match request {
        EditorRequest::SetCell {
            document_id,
            base_revision,
            ..
        }
        | EditorRequest::SetCells {
            document_id,
            base_revision,
            ..
        }
        | EditorRequest::AddRow {
            document_id,
            base_revision,
            ..
        }
        | EditorRequest::DeleteRow {
            document_id,
            base_revision,
            ..
        }
        | EditorRequest::AddColumn {
            document_id,
            base_revision,
            ..
        }
        | EditorRequest::DeleteColumn {
            document_id,
            base_revision,
            ..
        }
        | EditorRequest::SetColumnWidth {
            document_id,
            base_revision,
            ..
        }
        | EditorRequest::SetRowHeight {
            document_id,
            base_revision,
            ..
        }
        | EditorRequest::AddSheet {
            document_id,
            base_revision,
            ..
        }
        | EditorRequest::DeleteSheet {
            document_id,
            base_revision,
            ..
        }
        | EditorRequest::InsertImage {
            document_id,
            base_revision,
            ..
        }
        | EditorRequest::UpdateImage {
            document_id,
            base_revision,
            ..
        }
        | EditorRequest::DeleteImage {
            document_id,
            base_revision,
            ..
        }
        | EditorRequest::Undo {
            document_id,
            base_revision,
            ..
        }
        | EditorRequest::Redo {
            document_id,
            base_revision,
            ..
        } => Some((document_id, base_revision)),
        _ => None,
    };
    let Some((document_id, base_revision)) = context else {
        return Err(unexpected_reply("mutation request"));
    };
    if *document_id != current_document_id {
        return Err(crate::protocol::AppErrorDto {
            code: "document_changed".to_string(),
            message: "the workbook changed before the action could run".to_string(),
        });
    }
    *base_revision = current_revision;
    Ok(())
}

fn remove_committed_edits(
    pending: &mut crate::model::PendingCellEdits,
    committed: crate::model::PendingCellEdits,
) {
    for (coordinates, edit) in committed {
        if pending.get(&coordinates) == Some(&edit) {
            pending.remove(&coordinates);
        }
    }
}

fn set_busy(mut store: EditorStore, status: &str) {
    store.busy.set(true);
    store.error.set(None);
    store.status.set(status.to_string());
}

fn protocol_error(error: serde_json::Error) -> crate::protocol::AppErrorDto {
    crate::protocol::AppErrorDto {
        code: "protocol_error".to_string(),
        message: error.to_string(),
    }
}

fn unexpected_reply(action: &str) -> crate::protocol::AppErrorDto {
    crate::protocol::AppErrorDto {
        code: "protocol_error".to_string(),
        message: format!("unexpected {action} response"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{
        DocumentManifestView, EditorSessionView, EditorStateView, SheetExtentView, SheetLayoutView,
        SheetManifestView,
    };
    use crate::ports::editor::{EditorPort, PortFuture};
    use crate::protocol::{EditorResponse, ImageAnchorDto, ImageMarkerDto, SheetImageDto};
    use std::cell::RefCell;
    use std::collections::HashMap;

    fn session(revision: u64) -> EditorSessionView {
        EditorSessionView {
            document_id: 7,
            revision,
            editor_state: EditorStateView {
                can_undo: false,
                can_redo: false,
                is_dirty: false,
            },
        }
    }

    fn manifest(path: &str, file_name: &str) -> DocumentManifestView {
        DocumentManifestView {
            path: path.to_string(),
            file_name: file_name.to_string(),
            sheets: vec![SheetManifestView {
                name: "Sheet1".to_string(),
                extent: SheetExtentView {
                    row_count: 5,
                    column_count: 5,
                },
                layout: Rc::new(SheetLayoutView::default()),
            }],
        }
    }

    fn image(id: &str) -> SheetImageDto {
        SheetImageDto {
            id: id.to_string(),
            media_id: format!("media-{id}"),
            mime_type: "image/png".to_string(),
            intrinsic_width: 1,
            intrinsic_height: 1,
            anchor: ImageAnchorDto::OneCell {
                from: ImageMarkerDto {
                    row: 0,
                    col: 0,
                    row_offset_emu: 0,
                    col_offset_emu: 0,
                },
                width_emu: 9_525,
                height_emu: 9_525,
            },
            z_index: 0,
            renderable: false,
        }
    }

    #[test]
    fn committed_edits_do_not_remove_newer_input() {
        let coordinates = (0, 2, 3);
        let mut pending = HashMap::from([(coordinates, (2, "new".to_string()))]);
        let committed = HashMap::from([(coordinates, (1, "old".to_string()))]);

        remove_committed_edits(&mut pending, committed);

        assert_eq!(pending.get(&coordinates), Some(&(2, "new".to_string())));
    }

    #[test]
    fn committed_edits_remove_the_matching_generation() {
        let coordinates = (0, 2, 3);
        let edit = (1, "value".to_string());
        let mut pending = HashMap::from([(coordinates, edit.clone())]);

        remove_committed_edits(&mut pending, HashMap::from([(coordinates, edit)]));

        assert!(pending.is_empty());
    }

    #[test]
    fn viewport_is_clamped_to_small_and_scrolled_sheet_extents() {
        assert_eq!(
            clamp_viewport(
                SheetViewport::default(),
                SheetExtentView {
                    row_count: 5,
                    column_count: 5,
                },
            ),
            SheetViewport {
                row_start: 0,
                row_end: 5,
                col_start: 0,
                col_end: 5,
            }
        );
        assert_eq!(
            clamp_viewport(
                SheetViewport {
                    row_start: 99,
                    row_end: 135,
                    col_start: 19,
                    col_end: 35,
                },
                SheetExtentView {
                    row_count: 100,
                    column_count: 20,
                },
            ),
            SheetViewport {
                row_start: 99,
                row_end: 100,
                col_start: 19,
                col_end: 20,
            }
        );
    }

    #[test]
    fn saved_identity_updates_document_name_path_and_session() {
        let mut document = OpenDocumentView {
            document: manifest("/tmp/old.xlsx", "old.xlsx"),
            editor_session: session(1),
            initial_region: None,
        };
        merge_saved_document(
            &mut document,
            SavedDocumentView {
                document: None,
                identity: Some(crate::model::SavedDocumentIdentityView {
                    path: "/tmp/new.csv".to_string(),
                    file_name: "new.csv".to_string(),
                }),
                editor_session: session(2),
            },
        );

        assert_eq!(document.document.path, "/tmp/new.csv");
        assert_eq!(document.document.file_name, "new.csv");
        assert_eq!(document.editor_session.revision, 2);
    }

    fn patch(value: serde_json::Value) -> EditorPatchView {
        serde_json::from_value(value).expect("valid patch")
    }

    #[test]
    fn mutation_refresh_is_scoped_by_patch_and_active_sheet() {
        let cells = [patch(serde_json::json!({
            "type": "Cells",
            "data": { "changes": [] }
        }))];
        assert_eq!(
            MutationRefresh::for_patches(&cells, 0),
            MutationRefresh::default()
        );

        let layout = [patch(serde_json::json!({
            "type": "Layout",
            "data": { "patch": { "sheetIndex": 0 } }
        }))];
        assert_eq!(
            MutationRefresh::for_patches(&layout, 0),
            MutationRefresh {
                document: true,
                images: false,
            }
        );

        let other_sheet_image = [patch(serde_json::json!({
            "type": "ImageDeleted",
            "data": { "patch": { "sheetIndex": 1, "imageId": "image-1" } }
        }))];
        assert_eq!(
            MutationRefresh::for_patches(&other_sheet_image, 0),
            MutationRefresh::default()
        );

        let active_row = [patch(serde_json::json!({
            "type": "RowInserted",
            "data": { "patch": { "sheetIndex": 0, "rowIndex": 2, "count": 1 } }
        }))];
        assert_eq!(
            MutationRefresh::for_patches(&active_row, 0),
            MutationRefresh {
                document: true,
                images: true,
            }
        );
    }

    struct PagedImageEditor {
        offsets: Rc<RefCell<Vec<usize>>>,
    }

    impl EditorPort for PagedImageEditor {
        fn execute(&self, request: EditorRequest) -> PortFuture<EditorResponse> {
            let EditorRequest::SheetImages { offset, .. } = request else {
                panic!("unexpected request");
            };
            self.offsets.borrow_mut().push(offset);
            let response = match offset {
                0 => Ok(EditorReply::Images {
                    items: vec![image("first")],
                    next_offset: Some(256),
                }),
                256 => Ok(EditorReply::Images {
                    items: vec![image("second")],
                    next_offset: None,
                }),
                _ => panic!("unexpected image page offset"),
            };
            Box::pin(async move { response })
        }
    }

    #[test]
    fn image_catalog_follows_all_page_cursors() {
        let offsets = Rc::new(RefCell::new(Vec::new()));
        let editor = PagedImageEditor {
            offsets: Rc::clone(&offsets),
        };

        let items = futures::executor::block_on(load_image_catalog(&editor, 1, 2, 0)).unwrap();

        assert_eq!(*offsets.borrow(), vec![0, 256]);
        assert_eq!(
            items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "second"]
        );
    }

    #[cfg(feature = "desktop")]
    struct RecordingSaveEditor {
        prepared_target: Rc<RefCell<Option<String>>>,
        committed_path: Rc<RefCell<Option<String>>>,
    }

    #[cfg(feature = "desktop")]
    impl EditorPort for RecordingSaveEditor {
        fn execute(&self, request: EditorRequest) -> PortFuture<EditorResponse> {
            let response = match request {
                EditorRequest::PrepareSave { target_name, .. } => {
                    self.prepared_target.replace(Some(target_name));
                    Ok(EditorReply::SavePrepared {
                        save_token: "save-token".to_string(),
                        file_name: "selected.xlsx".to_string(),
                        bytes: vec![1, 2, 3],
                    })
                }
                EditorRequest::CommitSave { path, .. } => {
                    self.committed_path.replace(Some(path));
                    Ok(EditorReply::Saved {
                        value: serde_json::Value::Null,
                    })
                }
                request => panic!("unexpected request: {request:?}"),
            };
            Box::pin(async move { response })
        }
    }

    #[cfg(feature = "desktop")]
    struct RecordingFilePort {
        written_path: Rc<RefCell<Option<String>>>,
    }

    #[cfg(feature = "desktop")]
    impl crate::ports::file::FilePort for RecordingFilePort {
        fn choose_document_path(
            &self,
            _suggested_name: String,
            _mode: crate::ports::file::DocumentDialogMode,
        ) -> crate::ports::file::FileFuture<Result<Option<String>, crate::protocol::AppErrorDto>>
        {
            Box::pin(async { Ok(Some("/tmp/selected".to_string())) })
        }

        fn write_document_to_path(
            &self,
            path: String,
            bytes: Vec<u8>,
        ) -> crate::ports::file::FileFuture<Result<(), crate::protocol::AppErrorDto>> {
            assert_eq!(bytes, vec![1, 2, 3]);
            self.written_path.replace(Some(path));
            Box::pin(async { Ok(()) })
        }
    }

    #[cfg(feature = "desktop")]
    #[test]
    fn native_save_prepares_and_commits_the_selected_target() {
        let prepared_target = Rc::new(RefCell::new(None));
        let committed_path = Rc::new(RefCell::new(None));
        let written_path = Rc::new(RefCell::new(None));
        let ports = Rc::new(AppPorts {
            editor: Rc::new(RecordingSaveEditor {
                prepared_target: Rc::clone(&prepared_target),
                committed_path: Rc::clone(&committed_path),
            }),
            files: Rc::new(RecordingFilePort {
                written_path: Rc::clone(&written_path),
            }),
            operations: Rc::new(futures::lock::Mutex::new(())),
        });

        futures::executor::block_on(save_native(ports, 7, 1, "old.csv".to_string())).unwrap();

        assert_eq!(prepared_target.borrow().as_deref(), Some("/tmp/selected"));
        assert_eq!(written_path.borrow().as_deref(), Some("/tmp/selected.xlsx"));
        assert_eq!(
            committed_path.borrow().as_deref(),
            Some("/tmp/selected.xlsx")
        );
    }
}
