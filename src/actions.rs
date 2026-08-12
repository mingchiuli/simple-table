use std::rc::Rc;
use std::time::Duration;

use crate::protocol::{CellEdit, EditorReply, EditorRequest};
use base64::Engine;
use dioxus::prelude::{ReadableExt, WritableExt, spawn};
use dioxus_sdk_time::sleep;

use crate::model::{
    AppPorts, EditorMutationView, EditorStore, OpenDocumentView, SearchView, request_id,
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
    match ports
        .editor
        .execute(EditorRequest::DeleteLocalDocument { document_key })
        .await
    {
        Ok(_) => load_local_documents(store, ports).await,
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
    let result = match ports.editor.execute(request).await {
        Ok(EditorReply::Mutation { value }) => {
            match serde_json::from_value::<EditorMutationView>(value) {
                Ok(mutation) => {
                    store.accept_mutation(&mutation);
                    refresh_document(store, Rc::clone(&ports)).await;
                    if select_added_sheet {
                        select_last_sheet(store);
                    }
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
                    refresh_images(store, Rc::clone(&ports)).await;
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

    #[cfg(any(feature = "desktop", feature = "mobile"))]
    let result = save_native(
        store,
        Rc::clone(&ports),
        document_id,
        base_revision,
        target_name,
    )
    .await;

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
            if let Some(document) = store.document.write().as_mut().map(Rc::make_mut)
                && let Some(editor_session) = value.get("editorSession")
                && let Ok(session) = serde_json::from_value(editor_session.clone())
            {
                document.editor_session = session;
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

#[cfg(any(feature = "desktop", feature = "mobile"))]
async fn save_native(
    store: EditorStore,
    ports: Rc<AppPorts>,
    document_id: u64,
    base_revision: u64,
    target_name: String,
) -> Result<EditorReply, crate::protocol::AppErrorDto> {
    let save_token = request_id("save-native");
    let prepared = ports
        .editor
        .execute(EditorRequest::PrepareSave {
            request_id: save_token.clone(),
            document_id,
            base_revision,
            target_name: target_name.clone(),
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
    let path = match ports.files.write_document(file_name, bytes).await? {
        Some(path) => path,
        None => {
            let _ = ports
                .editor
                .execute(EditorRequest::AbortSave { save_token })
                .await;
            let mut store = store;
            store.busy.set(false);
            store.status.set("Save cancelled".to_string());
            return Ok(EditorReply::Empty);
        }
    };
    ports
        .editor
        .execute(EditorRequest::CommitSave { save_token, path })
        .await
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
    let target_name = document_name(store);
    let save_token = request_id("download");
    match ports
        .editor
        .execute(EditorRequest::PrepareSave {
            request_id: save_token,
            document_id,
            base_revision,
            target_name,
        })
        .await
    {
        Ok(EditorReply::SavePrepared {
            save_token,
            file_name,
            bytes,
        }) => {
            let write = ports.files.write_document(file_name, bytes).await;
            let _ = ports
                .editor
                .execute(EditorRequest::AbortSave { save_token })
                .await;
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
    refresh_region(store, Rc::clone(&ports), 0, 50, 0, 20).await;
    refresh_images(store, Rc::clone(&ports)).await;
}

pub async fn refresh_images(mut store: EditorStore, ports: Rc<AppPorts>) {
    let Some((document_id, base_revision)) = document_identity(store) else {
        return;
    };
    let sheet_index = store.active_sheet();
    let items = match ports
        .editor
        .execute(EditorRequest::SheetImages {
            document_id,
            base_revision,
            sheet_index,
            offset: 0,
            limit: 256,
        })
        .await
    {
        Ok(EditorReply::Images { items, .. }) => items,
        Ok(_) => {
            store.set_error(unexpected_reply("image catalog"));
            return;
        }
        Err(error) => {
            store.set_error(error);
            return;
        }
    };

    let mut assets = std::collections::HashMap::new();
    for image in &items {
        if !image.renderable {
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
    let generation = (*store.region_generation.read()).wrapping_add(1);
    store.region_generation.set(generation);
    store.viewport.set(crate::model::SheetViewport {
        row_start,
        row_end,
        col_start,
        col_end,
    });
    let response = ports
        .editor
        .execute(EditorRequest::Region {
            document_id,
            base_revision,
            sheet_index,
            row_start,
            row_end,
            col_start,
            col_end,
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

fn select_last_sheet(mut store: EditorStore) {
    let last_sheet = store.document.read().as_ref().map_or(0, |document| {
        document.document.sheets.len().saturating_sub(1)
    });
    store.active_sheet.set(last_sheet);
    store.selected_cell.set((0, 0));
    store.formula_text.set(String::new());
    store.viewport.set(crate::model::SheetViewport::default());
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
    use std::collections::HashMap;

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
}
