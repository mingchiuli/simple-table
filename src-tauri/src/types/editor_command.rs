use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Serialize, Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SetCellRequest {
    pub sheet_index: usize,
    pub row: usize,
    pub col: usize,
    pub text: String,
}
