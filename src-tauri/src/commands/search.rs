use super::{CommandExecutionRuntime, CommandU64};
use crate::error::AppError;
use crate::protocol_projection;
use crate::runtime::ApplicationRuntime;
use crate::types::{SearchResponse, SearchScope};
use tauri::State;

#[tauri::command(rename_all = "camelCase")]
pub async fn search(
    runtime: State<'_, ApplicationRuntime>,
    executions: State<'_, CommandExecutionRuntime>,
    document_id: CommandU64,
    base_revision: CommandU64,
    query: String,
    scope: SearchScope,
    current_sheet_index: Option<usize>,
) -> Result<SearchResponse, AppError> {
    let runtime = runtime.inner().clone();
    let scope = match scope {
        SearchScope::CurrentSheet => crate::domain::SearchScope::CurrentSheet,
        SearchScope::AllSheets => crate::domain::SearchScope::AllSheets,
    };
    executions
        .search()
        .run(move || {
            let outcome = runtime.search_queries().search(
                document_id.get(),
                base_revision.get(),
                &query,
                scope,
                current_sheet_index,
            )?;
            protocol_projection::search_response(outcome)
        })
        .await
}
