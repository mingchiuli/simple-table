mod region_loader;

use std::rc::Rc;
use std::time::Duration;

use crate::protocol::{
    CellEdit, EditorCommand, EditorReply, EditorRequest, FilterOperatorDto, ImageAnchorDto,
    SortDirectionDto,
};
use base64::Engine;
use dioxus::prelude::{ReadableExt, WritableExt, spawn};
use dioxus_sdk_time::sleep;
#[cfg(not(feature = "mobile"))]
use simple_table_web_protocol::{WebWorkspaceReply, WebWorkspaceRequest};

#[cfg(any(feature = "web", feature = "mobile"))]
use crate::model::LocalDocumentSummary;
use crate::model::{
    AppPorts, EditorMutationView, EditorPatchView, EditorStore, GridRenderWindow,
    GridScrollRequest, GridSelection, OpenDocumentView, SavedDocumentView, SheetRegionBoundsView,
    request_id,
};

pub(crate) use region_loader::RegionLoader;

pub enum MutationIntent {
    AddRow {
        sheet_index: usize,
        row_index: usize,
    },
    DeleteRow {
        sheet_index: usize,
        row_index: usize,
    },
    AddColumn {
        sheet_index: usize,
        col_index: usize,
    },
    DeleteColumn {
        sheet_index: usize,
        col_index: usize,
    },
    SortRows {
        sheet_index: usize,
        anchor_row: usize,
        anchor_col: usize,
        direction: SortDirectionDto,
    },
    SetFilter {
        sheet_index: usize,
        anchor_row: usize,
        col: usize,
        operator: FilterOperatorDto,
        value: String,
    },
    ClearFilter {
        sheet_index: usize,
        col: Option<usize>,
    },
    SetColumnWidth {
        sheet_index: usize,
        col_index: usize,
        width: Option<u32>,
    },
    SetRowHeight {
        sheet_index: usize,
        row_index: usize,
        height: Option<u32>,
    },
    AddSheet,
    DeleteSheet {
        sheet_index: usize,
    },
    InsertImage {
        sheet_index: usize,
        row: u32,
        col: u32,
        file_name: String,
        bytes: Vec<u8>,
    },
    UpdateImage {
        sheet_index: usize,
        image_id: String,
        anchor: ImageAnchorDto,
    },
    DeleteImage {
        sheet_index: usize,
        image_id: String,
    },
    Undo,
    Redo,
}

impl MutationIntent {
    fn status(&self) -> &'static str {
        match self {
            Self::Undo => "Undoing change",
            Self::Redo => "Redoing change",
            Self::SortRows { .. } => "Sorting rows",
            Self::SetFilter { .. } | Self::ClearFilter { .. } => "Updating filters",
            _ => "Applying changes",
        }
    }

    fn into_command(self, document_id: u64, base_revision: u64) -> EditorCommand {
        let (request, attachment) = match self {
            Self::AddRow {
                sheet_index,
                row_index,
            } => (
                EditorRequest::AddRow {
                    request_id: request_id("add-row"),
                    document_id,
                    base_revision,
                    sheet_index,
                    row_index,
                },
                None,
            ),
            Self::DeleteRow {
                sheet_index,
                row_index,
            } => (
                EditorRequest::DeleteRow {
                    request_id: request_id("delete-row"),
                    document_id,
                    base_revision,
                    sheet_index,
                    row_index,
                },
                None,
            ),
            Self::AddColumn {
                sheet_index,
                col_index,
            } => (
                EditorRequest::AddColumn {
                    request_id: request_id("add-column"),
                    document_id,
                    base_revision,
                    sheet_index,
                    col_index,
                },
                None,
            ),
            Self::DeleteColumn {
                sheet_index,
                col_index,
            } => (
                EditorRequest::DeleteColumn {
                    request_id: request_id("delete-column"),
                    document_id,
                    base_revision,
                    sheet_index,
                    col_index,
                },
                None,
            ),
            Self::SortRows {
                sheet_index,
                anchor_row,
                anchor_col,
                direction,
            } => (
                EditorRequest::SortRows {
                    request_id: request_id("sort"),
                    document_id,
                    base_revision,
                    sheet_index,
                    anchor_row,
                    anchor_col,
                    direction,
                },
                None,
            ),
            Self::SetFilter {
                sheet_index,
                anchor_row,
                col,
                operator,
                value,
            } => (
                EditorRequest::SetFilter {
                    request_id: request_id("set-filter"),
                    document_id,
                    base_revision,
                    sheet_index,
                    anchor_row,
                    col,
                    operator,
                    value,
                },
                None,
            ),
            Self::ClearFilter { sheet_index, col } => (
                EditorRequest::ClearFilter {
                    request_id: request_id("clear-filter"),
                    document_id,
                    base_revision,
                    sheet_index,
                    col,
                },
                None,
            ),
            Self::SetColumnWidth {
                sheet_index,
                col_index,
                width,
            } => (
                EditorRequest::SetColumnWidth {
                    request_id: request_id("column-width"),
                    document_id,
                    base_revision,
                    sheet_index,
                    col_index,
                    width,
                },
                None,
            ),
            Self::SetRowHeight {
                sheet_index,
                row_index,
                height,
            } => (
                EditorRequest::SetRowHeight {
                    request_id: request_id("row-height"),
                    document_id,
                    base_revision,
                    sheet_index,
                    row_index,
                    height,
                },
                None,
            ),
            Self::AddSheet => (
                EditorRequest::AddSheet {
                    request_id: request_id("add-sheet"),
                    document_id,
                    base_revision,
                },
                None,
            ),
            Self::DeleteSheet { sheet_index } => (
                EditorRequest::DeleteSheet {
                    request_id: request_id("delete-sheet"),
                    document_id,
                    base_revision,
                    sheet_index,
                },
                None,
            ),
            Self::InsertImage {
                sheet_index,
                row,
                col,
                file_name,
                bytes,
            } => (
                EditorRequest::InsertImage {
                    request_id: request_id("image"),
                    document_id,
                    base_revision,
                    sheet_index,
                    row,
                    col,
                    file_name,
                },
                Some(bytes),
            ),
            Self::UpdateImage {
                sheet_index,
                image_id,
                anchor,
            } => (
                EditorRequest::UpdateImage {
                    request_id: request_id("update-image"),
                    document_id,
                    base_revision,
                    sheet_index,
                    image_id,
                    anchor,
                },
                None,
            ),
            Self::DeleteImage {
                sheet_index,
                image_id,
            } => (
                EditorRequest::DeleteImage {
                    request_id: request_id("delete-image"),
                    document_id,
                    base_revision,
                    sheet_index,
                    image_id,
                },
                None,
            ),
            Self::Undo => (
                EditorRequest::Undo {
                    request_id: request_id("undo"),
                    document_id,
                    base_revision,
                },
                None,
            ),
            Self::Redo => (
                EditorRequest::Redo {
                    request_id: request_id("redo"),
                    document_id,
                    base_revision,
                },
                None,
            ),
        };
        EditorCommand {
            request,
            attachment,
        }
    }
}

#[cfg(feature = "mobile")]
const MOBILE_RECOVERY_ID: &str = "mobile-recovery";

pub async fn new_document(store: EditorStore, ports: Rc<AppPorts>) -> bool {
    let _operation = ports.operations.lock().await;
    let _busy = store.begin_operation("Creating workbook");
    match ports
        .editor
        .execute(EditorRequest::NewDocument {
            request_id: request_id("new"),
        })
        .await
    {
        Ok(reply) => {
            let opened = accept_document_reply(store, &ports.regions, reply);
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
    let _busy = store.begin_operation("Reading workbook");
    match ports
        .editor
        .execute_command(EditorCommand::with_attachment(
            EditorRequest::OpenDocument {
                request_id: request_id("open"),
                file_name,
            },
            bytes,
        ))
        .await
    {
        Ok(output) => {
            let opened = accept_document_reply(store, &ports.regions, output.reply);
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
    let _busy = store.begin_operation("Opening local workbook");

    #[cfg(feature = "mobile")]
    let response = if document_key == MOBILE_RECOVERY_ID {
        match ports.recovery.load().await {
            Ok(Some(recovery)) => ports
                .editor
                .execute_command(EditorCommand::with_attachment(
                    EditorRequest::OpenRecoveryDocument {
                        request_id: request_id("open-recovery"),
                        file_name: recovery.name,
                    },
                    recovery.bytes,
                ))
                .await
                .map(|output| output.reply),
            Ok(None) => Err(action_error(
                "mobile_recovery_missing",
                "the recovered workbook is no longer available",
            )),
            Err(error) => Err(error),
        }
    } else {
        Err(action_error(
            "mobile_recovery_missing",
            "the requested mobile workbook is unavailable",
        ))
    };

    #[cfg(not(feature = "mobile"))]
    let response = ports
        .workspace
        .execute(WebWorkspaceRequest::OpenLocalDocument {
            request_id: request_id("open-local"),
            document_key,
        })
        .await
        .and_then(|reply| match reply {
            WebWorkspaceReply::Document(value) => Ok(EditorReply::Document { value: Some(value) }),
            _ => Err(unexpected_reply("open local document")),
        });

    match response {
        Ok(reply) => {
            let opened = accept_document_reply(store, &ports.regions, reply);
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
        .workspace
        .execute(WebWorkspaceRequest::ListLocalDocuments)
        .await
    {
        Ok(WebWorkspaceReply::LocalDocuments(documents)) => {
            let mut store = store;
            store.local_documents.set(
                documents
                    .into_iter()
                    .map(|document| LocalDocumentSummary {
                        id: document.id,
                        name: document.name,
                        updated_at_ms: document.updated_at_ms,
                        has_recovery: document.has_recovery,
                    })
                    .collect(),
            );
        }
        Err(error) if error.code != "client_not_hydrated" => store.set_error(error),
        _ => {}
    }

    #[cfg(feature = "mobile")]
    match ports.recovery.load().await {
        Ok(Some(recovery)) => {
            let mut store = store;
            store.local_documents.set(vec![LocalDocumentSummary {
                id: MOBILE_RECOVERY_ID.to_string(),
                name: recovery.name,
                updated_at_ms: recovery.updated_at_ms,
                has_recovery: true,
            }]);
        }
        Ok(None) => {
            let mut store = store;
            store.local_documents.set(Vec::new());
        }
        Err(error) => store.set_error(error),
    }

    #[cfg(not(any(feature = "web", feature = "mobile")))]
    let _ = (store, ports);
}

pub async fn delete_local_document(store: EditorStore, ports: Rc<AppPorts>, document_key: String) {
    let _operation = ports.operations.lock().await;
    let _busy = store.begin_operation("Removing local workbook");

    #[cfg(feature = "mobile")]
    let response = if document_key == MOBILE_RECOVERY_ID {
        ports.recovery.clear().await.map(|()| EditorReply::Empty)
    } else {
        Err(action_error(
            "mobile_recovery_missing",
            "the requested mobile workbook is unavailable",
        ))
    };

    #[cfg(not(feature = "mobile"))]
    let response = ports
        .workspace
        .execute(WebWorkspaceRequest::DeleteLocalDocument {
            document_key: document_key.clone(),
        })
        .await
        .and_then(|reply| match reply {
            WebWorkspaceReply::Empty => Ok(EditorReply::Empty),
            _ => Err(unexpected_reply("delete local document")),
        });

    match response {
        Ok(EditorReply::Empty) => {
            let mut store = store;
            store
                .local_documents
                .write()
                .retain(|document| document.id != document_key);
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
        .insert((sheet_index, row, col), (generation, text.into()));
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
    let _busy = store.begin_operation("Applying changes");
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
            text: text.to_string(),
        })
        .collect();
    let result = run_mutation_locked(
        store,
        ports,
        EditorCommand::new(EditorRequest::SetCells {
            request_id: request_id("cells"),
            document_id,
            base_revision,
            changes: request_changes,
        }),
    )
    .await;
    if result.is_ok() {
        remove_committed_edits(&mut store.pending_edits.write(), changes);
    }
    result
}

pub async fn run_mutation(store: EditorStore, ports: Rc<AppPorts>, intent: MutationIntent) {
    let _operation = ports.operations.lock().await;
    let _busy = store.begin_operation(intent.status());
    if flush_pending_edits_locked(store, Rc::clone(&ports))
        .await
        .is_err()
    {
        return;
    }
    let Some((document_id, base_revision)) = document_identity(store) else {
        let error = crate::protocol::AppErrorDto {
            code: "no_document".to_string(),
            message: "no workbook is open".to_string(),
        };
        store.set_error(error);
        return;
    };
    let command = intent.into_command(document_id, base_revision);
    let _ = run_mutation_locked(store, Rc::clone(&ports), command).await;
}

async fn run_mutation_locked(
    mut store: EditorStore,
    ports: Rc<AppPorts>,
    command: EditorCommand,
) -> Result<(), crate::protocol::AppErrorDto> {
    let select_added_sheet = matches!(&command.request, EditorRequest::AddSheet { .. });
    let previous_sheet_name = active_sheet_name(store);
    let result = match ports.editor.execute_command(command).await {
        Ok(crate::protocol::EditorOutput {
            reply: EditorReply::Mutation { value },
            ..
        }) => {
            let mutation: EditorMutationView = value.into();
            let Some((document_id, revision)) = document_identity(store) else {
                let error = crate::protocol::AppErrorDto {
                    code: "document_closed".to_string(),
                    message: "the workbook is no longer open".to_string(),
                };
                store.set_error(error.clone());
                return Err(error);
            };
            if mutation.document_id != document_id || mutation.revision < revision {
                let error = crate::protocol::AppErrorDto {
                    code: "stale_mutation_response".to_string(),
                    message: "the mutation response did not match the current workbook revision"
                        .to_string(),
                };
                store.set_error(error.clone());
                return Err(error);
            }
            let refresh = MutationRefresh::for_patches(&mutation.patches, store.active_sheet());
            if refresh.document {
                ports.regions.reset();
            }
            store.accept_mutation(mutation);
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
            schedule_current_window(store, &ports);
            sync_formula_text(store);
            if refresh.images || select_added_sheet {
                refresh_images(store, Rc::clone(&ports)).await;
            }
            schedule_recovery(store, ports);
            Ok(())
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
    run_mutation(store, ports, MutationIntent::Undo).await;
}

pub async fn redo(store: EditorStore, ports: Rc<AppPorts>) {
    run_mutation(store, ports, MutationIntent::Redo).await;
}

pub async fn save_local(store: EditorStore, ports: Rc<AppPorts>) {
    save_local_to_target(store, ports, None).await;
}

#[cfg(feature = "mobile")]
pub async fn save_local_as(store: EditorStore, ports: Rc<AppPorts>, target_name: String) {
    save_local_to_target(store, ports, Some(target_name)).await;
}

async fn save_local_to_target(
    store: EditorStore,
    ports: Rc<AppPorts>,
    target_name: Option<String>,
) {
    let _operation = ports.operations.lock().await;
    let _busy = store.begin_operation("Saving workbook");
    if flush_pending_edits_locked(store, Rc::clone(&ports))
        .await
        .is_err()
    {
        return;
    }
    let Some((document_id, base_revision)) = document_identity(store) else {
        return;
    };
    let target_name = target_name.unwrap_or_else(|| document_name(store));
    #[cfg(feature = "mobile")]
    let existing_target = document_path(store);
    #[cfg(feature = "web")]
    let result = ports
        .workspace
        .execute(WebWorkspaceRequest::SaveLocal {
            request_id: request_id("save-local"),
            document_id,
            base_revision,
            target_name,
        })
        .await
        .and_then(|reply| match reply {
            WebWorkspaceReply::Saved(value) => Ok(EditorReply::Saved { value }),
            WebWorkspaceReply::Empty => Ok(EditorReply::Empty),
            _ => Err(unexpected_reply("save local document")),
        });

    #[cfg(feature = "desktop")]
    let result = save_native(Rc::clone(&ports), document_id, base_revision, target_name).await;

    #[cfg(all(feature = "mobile", not(feature = "desktop")))]
    let result = save_mobile(
        Rc::clone(&ports),
        document_id,
        base_revision,
        target_name,
        existing_target,
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
            accept_saved_document(store, value.into());
            #[cfg(feature = "mobile")]
            let _ = ports.recovery.clear().await;
            let mut store = store;
            store.status.set("Saved".to_string());
        }
        Ok(EditorReply::Empty) => {
            let mut store = store;
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
        .execute_command(EditorCommand::new(EditorRequest::PrepareSave {
            request_id: save_token.clone(),
            document_id,
            base_revision,
            target_name: path.clone(),
        }))
        .await?;
    let EditorReply::SavePrepared {
        save_token,
        file_name,
    } = prepared.reply
    else {
        return Err(unexpected_reply("prepare save"));
    };
    let bytes = prepared
        .attachment
        .ok_or_else(|| unexpected_reply("prepare save bytes"))?;
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
    existing_target: Option<String>,
) -> Result<EditorReply, crate::protocol::AppErrorDto> {
    let save_token = request_id("save-mobile");
    let prepared = ports
        .editor
        .execute_command(EditorCommand::new(EditorRequest::PrepareSave {
            request_id: save_token.clone(),
            document_id,
            base_revision,
            target_name,
        }))
        .await?;
    let EditorReply::SavePrepared {
        save_token,
        file_name,
    } = prepared.reply
    else {
        return Err(unexpected_reply("prepare mobile save"));
    };
    let bytes = prepared
        .attachment
        .ok_or_else(|| unexpected_reply("prepare mobile save bytes"))?;
    let path = match ports
        .files
        .write_document_to_target(existing_target, file_name, bytes)
        .await
    {
        Ok(Some(path)) => path,
        Ok(None) => {
            let _ = ports
                .editor
                .execute(EditorRequest::AbortSave { save_token })
                .await;
            return Ok(EditorReply::Empty);
        }
        Err(error) => {
            let _ = ports
                .editor
                .execute(EditorRequest::AbortSave { save_token })
                .await;
            return Err(error);
        }
    };
    ports
        .editor
        .execute(EditorRequest::CommitSave { save_token, path })
        .await
}

pub async fn download_copy(mut store: EditorStore, ports: Rc<AppPorts>) {
    if store.busy() {
        return;
    }
    let _operation = ports.operations.lock().await;
    let _busy = store.begin_operation("Preparing copy");
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
        .execute_command(EditorCommand::new(EditorRequest::PrepareExport {
            document_id,
            base_revision,
            target_name: target_name.clone(),
        }))
        .await
    {
        Ok(output) => {
            let EditorReply::ExportPrepared { file_name } = output.reply else {
                store.set_error(unexpected_reply("download"));
                return;
            };
            let Some(bytes) = output.attachment else {
                store.set_error(unexpected_reply("download bytes"));
                return;
            };
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
                    store.status.set("Copy downloaded".to_string());
                }
                #[cfg(feature = "mobile")]
                Ok(None) => {
                    store.status.set("Copy sent to device".to_string());
                }
                #[cfg(not(feature = "mobile"))]
                Ok(None) => {}
                Err(error) => store.set_error(error),
            }
        }
        Err(error) => store.set_error(error),
    }
}

pub async fn search(store: EditorStore, ports: Rc<AppPorts>, query: String, all_sheets: bool) {
    let _operation = ports.operations.lock().await;
    let _busy = store.begin_operation("Searching workbook");
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
        Ok(EditorReply::Search { value }) => {
            let mut store = store;
            store.search.set(Some(value.into()));
            store.search_open.set(true);
            store.status.set("Search complete".to_string());
        }
        Ok(_) => store.set_error(unexpected_reply("search")),
        Err(error) => store.set_error(error),
    }
}

pub async fn select_sheet(mut store: EditorStore, ports: Rc<AppPorts>, sheet_index: usize) {
    let _operation = ports.operations.lock().await;
    let _busy = store.begin_operation("Switching worksheet");
    if flush_pending_edits_locked(store, Rc::clone(&ports))
        .await
        .is_err()
    {
        return;
    }
    store.active_sheet.set(sheet_index);
    store.selection.set(GridSelection {
        sheet_index,
        ..GridSelection::default()
    });
    store.render_window.set(GridRenderWindow {
        sheet_index,
        ..GridRenderWindow::default()
    });
    store.formula_text.set(String::new());
    store.grid_scroll_request.set(Some(GridScrollRequest {
        sheet_index,
        row: 0,
        col: 0,
        focus: false,
    }));
    refresh_images(store, Rc::clone(&ports)).await;
    store.status.set("Ready".to_string());
}

pub async fn select_search_result(
    mut store: EditorStore,
    ports: Rc<AppPorts>,
    sheet_index: usize,
    row: usize,
    col: usize,
) {
    let _operation = ports.operations.lock().await;
    let _busy = store.begin_operation("Opening search result");
    if flush_pending_edits_locked(store, Rc::clone(&ports))
        .await
        .is_err()
    {
        return;
    }
    let row_start = row.saturating_sub(6);
    let col_start = col.saturating_sub(4);
    store.active_sheet.set(sheet_index);
    store.selection.set(GridSelection {
        sheet_index,
        row,
        col,
        merge: None,
    });
    store.formula_text.set(String::new());
    store.grid_scroll_request.set(Some(GridScrollRequest {
        sheet_index,
        row: row_start,
        col: col_start,
        focus: false,
    }));
    if let Some(extent) = sheet_extent(store, sheet_index)
        && let Err(error) = ports
            .regions
            .ensure_region(
                store,
                SheetRegionBoundsView {
                    sheet_index,
                    row_start: row,
                    row_end: row.saturating_add(1),
                    col_start: col,
                    col_end: col.saturating_add(1),
                },
                extent,
            )
            .await
    {
        store.set_error(error);
        return;
    }
    store.select_cell(sheet_index, row, col);
    let (row, col) = store.selected_cell();
    store
        .formula_text
        .set(store.cell_edit_text(sheet_index, row, col));
    refresh_images(store, Rc::clone(&ports)).await;
    store.status.set("Ready".to_string());
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
            .execute_command(EditorCommand::new(EditorRequest::ImageBytes {
                document_id,
                base_revision,
                sheet_index,
                image_id: image.id.clone(),
            }))
            .await
        {
            Ok(output) if matches!(output.reply, EditorReply::Bytes) => {
                let Some(bytes) = output.attachment else {
                    store.set_error(unexpected_reply("image bytes"));
                    return;
                };
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
    if document_identity(store).is_none() {
        return true;
    }
    let _busy = store.begin_operation("Closing workbook");
    if flush_pending_edits_locked(store, Rc::clone(&ports))
        .await
        .is_err()
    {
        return false;
    }
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
            #[cfg(feature = "mobile")]
            let _ = ports.recovery.clear().await;
            store.document.set(None);
            store.region_cache.write().clear();
            ports.regions.reset();
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

async fn refresh_document(store: EditorStore, ports: Rc<AppPorts>) {
    match ports.editor.execute(EditorRequest::ActiveDocument).await {
        Ok(EditorReply::Document { value: Some(value) }) => store.refresh_document(value.into()),
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
                EditorPatchView::Layout { .. } => {}
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
                EditorPatchView::SheetInserted
                | EditorPatchView::SheetDeleted
                | EditorPatchView::SheetsReplaced
                | EditorPatchView::ResyncRequired => {
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
        store.select_cell(store.active_sheet(), clamped.0, clamped.1);
        store.grid_scroll_request.set(Some(GridScrollRequest {
            sheet_index: store.active_sheet(),
            row: clamped.0,
            col: clamped.1,
            focus: false,
        }));
    }
}

fn sync_formula_text(mut store: EditorStore) {
    let sheet_index = store.active_sheet();
    let selected = store.selected_cell();
    let value = store.cell_edit_text(sheet_index, selected.0, selected.1);
    store.formula_text.set(value);
}

fn select_last_sheet(mut store: EditorStore) {
    let last_sheet = store.document.read().as_ref().map_or(0, |document| {
        document.document.sheets.len().saturating_sub(1)
    });
    store.active_sheet.set(last_sheet);
    store.selection.set(GridSelection {
        sheet_index: last_sheet,
        ..GridSelection::default()
    });
    store.formula_text.set(String::new());
    store.render_window.set(GridRenderWindow {
        sheet_index: last_sheet,
        ..GridRenderWindow::default()
    });
    store.grid_scroll_request.set(Some(GridScrollRequest {
        sheet_index: last_sheet,
        row: 0,
        col: 0,
        focus: false,
    }));
}

fn reset_current_sheet_viewport(mut store: EditorStore) {
    let sheet_index = store.active_sheet();
    store.selection.set(GridSelection {
        sheet_index,
        ..GridSelection::default()
    });
    store.formula_text.set(String::new());
    store.render_window.set(GridRenderWindow {
        sheet_index,
        ..GridRenderWindow::default()
    });
    store.grid_scroll_request.set(Some(GridScrollRequest {
        sheet_index,
        row: 0,
        col: 0,
        focus: false,
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
                    .workspace
                    .execute(WebWorkspaceRequest::CheckpointRecovery {
                        request_id: request_id("recovery"),
                        document_id,
                        base_revision,
                        target_name: document_name(store),
                    })
                    .await;
            } else {
                let _ = ports
                    .workspace
                    .execute(WebWorkspaceRequest::ClearRecovery)
                    .await;
            }
        });
    }

    #[cfg(feature = "mobile")]
    {
        let generation = store.edit_generation().wrapping_add(1);
        let mut store = store;
        store.edit_generation.set(generation);
        spawn(async move {
            sleep(Duration::from_secs(2)).await;
            if store.edit_generation() != generation {
                return;
            }

            let _operation = ports.operations.lock().await;
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
            if !is_dirty {
                let _ = ports.recovery.clear().await;
                return;
            }

            let prepared = ports
                .editor
                .execute_command(EditorCommand::new(EditorRequest::PrepareExport {
                    document_id,
                    base_revision,
                    target_name: document_name(store),
                }))
                .await;
            if store.edit_generation() != generation
                || document_identity(store) != Some((document_id, base_revision))
            {
                return;
            }
            if let Ok(output) = prepared
                && let EditorReply::ExportPrepared { file_name } = output.reply
                && let Some(bytes) = output.attachment
            {
                let _ = ports.recovery.checkpoint(file_name, bytes).await;
            }
        });
    }

    #[cfg(not(any(feature = "web", feature = "mobile")))]
    let _ = (store, ports);
}

fn accept_document_reply(
    mut store: EditorStore,
    regions: &RegionLoader,
    reply: EditorReply,
) -> bool {
    let EditorReply::Document { value } = reply else {
        store.set_error(unexpected_reply("document"));
        return false;
    };
    let Some(value) = value else {
        regions.reset();
        store.document.set(None);
        store.region_cache.write().clear();
        return false;
    };
    regions.reset();
    store.accept_document(value.into());
    true
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

#[cfg(feature = "mobile")]
fn document_path(store: EditorStore) -> Option<String> {
    store
        .document
        .read()
        .as_ref()
        .map(|document| document.document.path.clone())
        .filter(|path| {
            #[cfg(target_os = "android")]
            return path.starts_with("content://");

            #[cfg(not(target_os = "android"))]
            !path.is_empty()
        })
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

fn sheet_extent(store: EditorStore, sheet_index: usize) -> Option<crate::model::SheetExtentView> {
    store
        .document
        .peek()
        .as_ref()?
        .document
        .sheets
        .get(sheet_index)
        .map(|sheet| sheet.extent)
}

fn schedule_current_window(store: EditorStore, ports: &AppPorts) {
    let sheet_index = store.active_sheet();
    let Some(extent) = sheet_extent(store, sheet_index) else {
        return;
    };
    let visible_rows = store.visible_rows(sheet_index, extent.row_count.max(1));
    let window =
        store
            .render_window
            .peek()
            .clamped(sheet_index, visible_rows.len(), extent.column_count);
    if visible_rows.len() == extent.row_count.max(1) {
        ports
            .regions
            .schedule_viewport(store, window.bounds(), extent);
    } else {
        ports.regions.schedule_visible_rows(
            store,
            sheet_index,
            visible_rows
                .get(window.row_start..window.row_end.min(visible_rows.len()))
                .unwrap_or_default(),
            window.col_start,
            window.col_end,
            extent,
        );
    }
}

#[cfg(feature = "desktop")]
fn path_for_prepared_name(selected_path: String, prepared_name: &str) -> String {
    let mut path = std::path::PathBuf::from(&selected_path);
    if path.file_name().and_then(|name| name.to_str()) != Some(prepared_name) {
        path.set_file_name(prepared_name);
    }
    path.to_string_lossy().into_owned()
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

#[cfg(feature = "mobile")]
fn action_error(code: &str, message: &str) -> crate::protocol::AppErrorDto {
    crate::protocol::AppErrorDto {
        code: code.to_string(),
        message: message.to_string(),
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
    #[cfg(any(feature = "desktop", feature = "mobile"))]
    use crate::protocol::{EditorCommand, EditorOutput, SavedDocumentResponse};
    use crate::protocol::{ImageAnchorDto, ImageMarkerDto, SheetImageDto};
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
                history: Default::default(),
            },
            capabilities: Default::default(),
            formula_status: Default::default(),
            filters: Vec::new(),
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
        let mut pending = HashMap::from([(coordinates, (2, Rc::<str>::from("new")))]);
        let committed = HashMap::from([(coordinates, (1, Rc::<str>::from("old")))]);

        remove_committed_edits(&mut pending, committed);

        assert_eq!(
            pending
                .get(&coordinates)
                .map(|(generation, text)| (*generation, text.as_ref())),
            Some((2, "new"))
        );
    }

    #[test]
    fn committed_edits_remove_the_matching_generation() {
        let coordinates = (0, 2, 3);
        let edit = (1, Rc::<str>::from("value"));
        let mut pending = HashMap::from([(coordinates, edit.clone())]);

        remove_committed_edits(&mut pending, HashMap::from([(coordinates, edit)]));

        assert!(pending.is_empty());
    }

    #[test]
    fn mutation_intent_binds_current_context_and_keeps_binary_out_of_the_request() {
        let command = MutationIntent::InsertImage {
            sheet_index: 2,
            row: 3,
            col: 4,
            file_name: "chart.png".to_string(),
            bytes: vec![1, 2, 3],
        }
        .into_command(17, 29);

        assert_eq!(command.attachment, Some(vec![1, 2, 3]));
        assert!(matches!(
            command.request,
            EditorRequest::InsertImage {
                document_id: 17,
                base_revision: 29,
                sheet_index: 2,
                row: 3,
                col: 4,
                ..
            }
        ));
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

    #[test]
    fn mutation_refresh_is_scoped_by_patch_and_active_sheet() {
        let cells = [EditorPatchView::Cells {
            changes: Vec::new(),
        }];
        assert_eq!(
            MutationRefresh::for_patches(&cells, 0),
            MutationRefresh::default()
        );

        let layout = [EditorPatchView::Layout {
            patch: crate::model::LayoutPatchView {
                sheet_index: 0,
                column_widths: HashMap::new(),
                row_heights: HashMap::new(),
            },
        }];
        assert_eq!(
            MutationRefresh::for_patches(&layout, 0),
            MutationRefresh::default()
        );

        let other_sheet_image = [EditorPatchView::ImageDeleted {
            patch: crate::model::SheetPatchView { sheet_index: 1 },
        }];
        assert_eq!(
            MutationRefresh::for_patches(&other_sheet_image, 0),
            MutationRefresh::default()
        );

        let active_row = [EditorPatchView::RowInserted {
            patch: crate::model::SheetPatchView { sheet_index: 0 },
        }];
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
        fn execute(
            &self,
            request: EditorRequest,
        ) -> PortFuture<Result<EditorReply, crate::protocol::AppErrorDto>> {
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
        fn execute(
            &self,
            request: EditorRequest,
        ) -> PortFuture<Result<EditorReply, crate::protocol::AppErrorDto>> {
            let response = match request {
                EditorRequest::CommitSave { path, .. } => {
                    self.committed_path.replace(Some(path));
                    Ok(EditorReply::Saved {
                        value: saved_response(),
                    })
                }
                request => panic!("unexpected request: {request:?}"),
            };
            Box::pin(async move { response })
        }

        fn execute_command(
            &self,
            command: EditorCommand,
        ) -> PortFuture<crate::protocol::EditorResponse> {
            let EditorCommand {
                request,
                attachment,
            } = command;
            assert!(attachment.is_none());
            let EditorRequest::PrepareSave { target_name, .. } = request else {
                panic!("unexpected command request: {request:?}");
            };
            self.prepared_target.replace(Some(target_name));
            Box::pin(async {
                Ok(EditorOutput::with_attachment(
                    EditorReply::SavePrepared {
                        save_token: "save-token".to_string(),
                        file_name: "selected.xlsx".to_string(),
                    },
                    vec![1, 2, 3],
                ))
            })
        }
    }

    #[cfg(any(feature = "desktop", feature = "mobile"))]
    fn saved_response() -> SavedDocumentResponse {
        SavedDocumentResponse {
            document: None,
            identity: None,
            editor_session: crate::protocol::EditorSessionInfo {
                document_id: 7,
                revision: 2,
                editor_state: crate::protocol::EditorStateInfo {
                    can_undo: false,
                    can_redo: false,
                    is_dirty: false,
                    history: Default::default(),
                },
                capabilities: Default::default(),
                formula_status: crate::protocol::FormulaStatus::Ready {
                    diagnostics: Default::default(),
                },
                filters: Vec::new(),
            },
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
        let editor: Rc<dyn EditorPort> = Rc::new(RecordingSaveEditor {
            prepared_target: Rc::clone(&prepared_target),
            committed_path: Rc::clone(&committed_path),
        });
        let ports = Rc::new(AppPorts {
            regions: RegionLoader::new(Rc::clone(&editor)),
            editor,
            files: Rc::new(RecordingFilePort {
                written_path: Rc::clone(&written_path),
            }),
            workspace: crate::ports::workspace::platform_workspace_port(),
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

    #[cfg(feature = "mobile")]
    struct RecordingMobileSaveEditor {
        prepared_target: Rc<RefCell<Option<String>>>,
        committed_path: Rc<RefCell<Option<String>>>,
    }

    #[cfg(feature = "mobile")]
    impl EditorPort for RecordingMobileSaveEditor {
        fn execute(
            &self,
            request: EditorRequest,
        ) -> PortFuture<Result<EditorReply, crate::protocol::AppErrorDto>> {
            let response = match request {
                EditorRequest::CommitSave { path, .. } => {
                    self.committed_path.replace(Some(path));
                    Ok(EditorReply::Saved {
                        value: saved_response(),
                    })
                }
                request => panic!("unexpected request: {request:?}"),
            };
            Box::pin(async move { response })
        }

        fn execute_command(
            &self,
            command: EditorCommand,
        ) -> PortFuture<crate::protocol::EditorResponse> {
            let EditorCommand {
                request,
                attachment,
            } = command;
            assert!(attachment.is_none());
            let EditorRequest::PrepareSave { target_name, .. } = request else {
                panic!("unexpected command request: {request:?}");
            };
            self.prepared_target.replace(Some(target_name));
            Box::pin(async {
                Ok(EditorOutput::with_attachment(
                    EditorReply::SavePrepared {
                        save_token: "save-token".to_string(),
                        file_name: "quarterly plan.xlsx".to_string(),
                    },
                    vec![1, 2, 3],
                ))
            })
        }
    }

    #[cfg(feature = "mobile")]
    struct RecordingMobileFilePort {
        written_name: Rc<RefCell<Option<String>>>,
    }

    #[cfg(feature = "mobile")]
    impl crate::ports::file::FilePort for RecordingMobileFilePort {
        fn pick_file(
            &self,
            _kind: crate::ports::file::MobileFileKind,
        ) -> crate::ports::file::FileFuture<
            Result<Option<crate::ports::file::PickedFile>, crate::protocol::AppErrorDto>,
        > {
            Box::pin(async { Ok(None) })
        }

        fn write_document(
            &self,
            _suggested_name: String,
            _bytes: Vec<u8>,
        ) -> crate::ports::file::FileFuture<Result<Option<String>, crate::protocol::AppErrorDto>>
        {
            Box::pin(async { Ok(None) })
        }

        fn write_document_to_target(
            &self,
            existing_target: Option<String>,
            suggested_name: String,
            bytes: Vec<u8>,
        ) -> crate::ports::file::FileFuture<Result<Option<String>, crate::protocol::AppErrorDto>>
        {
            assert_eq!(existing_target, None);
            assert_eq!(bytes, vec![1, 2, 3]);
            self.written_name.replace(Some(suggested_name));
            Box::pin(async { Ok(Some("content://documents/quarterly-plan".to_string())) })
        }
    }

    #[cfg(feature = "mobile")]
    #[test]
    fn mobile_save_uses_the_requested_name_and_commits_the_written_target() {
        let prepared_target = Rc::new(RefCell::new(None));
        let committed_path = Rc::new(RefCell::new(None));
        let written_name = Rc::new(RefCell::new(None));
        let editor: Rc<dyn EditorPort> = Rc::new(RecordingMobileSaveEditor {
            prepared_target: Rc::clone(&prepared_target),
            committed_path: Rc::clone(&committed_path),
        });
        let ports = Rc::new(AppPorts {
            regions: RegionLoader::new(Rc::clone(&editor)),
            editor,
            files: Rc::new(RecordingMobileFilePort {
                written_name: Rc::clone(&written_name),
            }),
            recovery: crate::ports::recovery::platform_recovery_port(),
            operations: Rc::new(futures::lock::Mutex::new(())),
        });

        futures::executor::block_on(save_mobile(ports, 7, 1, "quarterly plan".to_string(), None))
            .unwrap();

        assert_eq!(prepared_target.borrow().as_deref(), Some("quarterly plan"));
        assert_eq!(
            written_name.borrow().as_deref(),
            Some("quarterly plan.xlsx")
        );
        assert_eq!(
            committed_path.borrow().as_deref(),
            Some("content://documents/quarterly-plan")
        );
    }
}
