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
                refresh_images(store, ports).await;
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
                refresh_images(store, ports).await;
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
                refresh_images(store, ports).await;
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
            flush_pending_edits(store, ports).await;
        }
    });
}

pub async fn flush_pending_edits(mut store: EditorStore, ports: Rc<AppPorts>) {
    let changes = std::mem::take(&mut *store.pending_edits.write());
    if changes.is_empty() {
        return;
    }
    let Some((document_id, base_revision)) = document_identity(store) else {
        return;
    };
    let changes = changes
        .into_iter()
        .map(|((sheet_index, row, col), (_, text))| CellEdit {
            sheet_index,
            row,
            col,
            text,
        })
        .collect();
    run_mutation(
        store,
        Rc::clone(&ports),
        EditorRequest::SetCells {
            request_id: request_id("cells"),
            document_id,
            base_revision,
            changes,
        },
    )
    .await;
}

pub async fn run_mutation(mut store: EditorStore, ports: Rc<AppPorts>, request: EditorRequest) {
    store.busy.set(true);
    match ports.editor.execute(request).await {
        Ok(EditorReply::Mutation { value }) => {
            match serde_json::from_value::<EditorMutationView>(value) {
                Ok(mutation) => {
                    store.accept_mutation(&mutation);
                    refresh_document(store, Rc::clone(&ports)).await;
                    refresh_region(store, Rc::clone(&ports), 0, 50, 0, 20).await;
                    refresh_images(store, Rc::clone(&ports)).await;
                    schedule_recovery(store, ports);
                }
                Err(error) => store.set_error(protocol_error(error)),
            }
        }
        Ok(_) => store.set_error(unexpected_reply("mutation")),
        Err(error) => store.set_error(error),
    }
}

pub async fn undo(store: EditorStore, ports: Rc<AppPorts>) {
    flush_pending_edits(store, Rc::clone(&ports)).await;
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
    flush_pending_edits(store, Rc::clone(&ports)).await;
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
    flush_pending_edits(store, Rc::clone(&ports)).await;
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
    flush_pending_edits(store, Rc::clone(&ports)).await;
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
                Ok(None) => {}
                Err(error) => store.set_error(error),
            }
        }
        Ok(_) => store.set_error(unexpected_reply("download")),
        Err(error) => store.set_error(error),
    }
}

pub async fn search(store: EditorStore, ports: Rc<AppPorts>, query: String, all_sheets: bool) {
    flush_pending_edits(store, Rc::clone(&ports)).await;
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
    flush_pending_edits(store, Rc::clone(&ports)).await;
    store.active_sheet.set(sheet_index);
    store.selected_cell.set((0, 0));
    refresh_region(store, Rc::clone(&ports), 0, 50, 0, 20).await;
    refresh_images(store, ports).await;
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

pub async fn close_document(mut store: EditorStore, ports: Rc<AppPorts>) {
    flush_pending_edits(store, Rc::clone(&ports)).await;
    let Some((document_id, base_revision)) = document_identity(store) else {
        return;
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
        }
        Ok(_) => store.set_error(unexpected_reply("close")),
        Err(error) => store.set_error(error),
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
    match ports
        .editor
        .execute(EditorRequest::Region {
            document_id,
            base_revision,
            sheet_index: store.active_sheet(),
            row_start,
            row_end,
            col_start,
            col_end,
        })
        .await
    {
        Ok(EditorReply::Region { value }) => match serde_json::from_value(value) {
            Ok(region) => store.region.set(Some(region)),
            Err(error) => store.set_error(protocol_error(error)),
        },
        Ok(_) => store.set_error(unexpected_reply("region")),
        Err(error) => store.set_error(error),
    }
}

async fn refresh_document(store: EditorStore, ports: Rc<AppPorts>) {
    match ports.editor.execute(EditorRequest::ActiveDocument).await {
        Ok(reply) => {
            accept_document_reply(store, reply);
        }
        Err(error) => store.set_error(error),
    }
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
            if !store
                .document
                .read()
                .as_ref()
                .is_some_and(|document| document.editor_session.editor_state.is_dirty)
            {
                return;
            }
            let _ = ports
                .editor
                .execute(EditorRequest::CheckpointRecovery {
                    request_id: request_id("recovery"),
                    document_id,
                    base_revision,
                    target_name: document_name(store),
                })
                .await;
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
