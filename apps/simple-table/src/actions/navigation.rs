use std::rc::Rc;

use dioxus::prelude::WritableExt;

use super::images::refresh_images;
use super::mutation::flush_pending_edits_locked;
use super::shared::{document_identity, sheet_extent, unexpected_reply};
use crate::model::{
    AppPorts, EditorStore, GridRenderWindow, GridScrollRequest, GridSelection,
    SheetRegionBoundsView,
};
use crate::protocol::{EditorReply, EditorRequest};

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
