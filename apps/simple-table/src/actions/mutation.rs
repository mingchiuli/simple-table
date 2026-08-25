use std::rc::Rc;
use std::time::Duration;

use dioxus::prelude::{ReadableExt, WritableExt, spawn};
use dioxus_sdk_time::sleep;

use super::images::refresh_images;
use super::recovery;
use super::shared::{
    active_sheet_name, document_identity, refresh_document, schedule_current_window,
    unexpected_reply,
};
use crate::model::{
    AppPorts, EditorMutationView, EditorPatchView, EditorStore, GridRenderWindow,
    GridScrollRequest, GridSelection, request_id,
};
use crate::protocol::{
    CellEdit, EditorCommand, EditorReply, EditorRequest, FilterOperatorDto, ImageAnchorDto,
    SortDirectionDto,
};

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

pub(super) async fn flush_pending_edits_locked(
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
        store.set_error(crate::protocol::AppErrorDto {
            code: "no_document".to_string(),
            message: "no workbook is open".to_string(),
        });
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
            recovery::schedule(store, ports);
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
                EditorPatchView::Cells { .. } | EditorPatchView::Layout { .. } => {}
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

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
}
