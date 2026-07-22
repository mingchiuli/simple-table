use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::domain::CellValue as DomainCellValue;

use super::cell::{CellFormatProjection, CellValue, CellValueProjection};

#[derive(Serialize, Deserialize, TS, Clone, Debug, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct CellStyleProjection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background_color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub italic: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub horizontal_align: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vertical_align: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number_format: Option<String>,
}

/// A cell change with its Sheet coordinates for document regions and mutations.
#[derive(Deserialize, TS, Clone, Debug)]
#[serde(rename_all = "camelCase")]
#[ts(rename_all = "camelCase")]
pub struct SheetCellChange {
    pub sheet_index: usize,
    pub row: usize,
    pub col: usize,
    pub value: CellValue,
    #[serde(default, skip)]
    #[ts(skip)]
    pub display: Option<String>,
    #[serde(default, skip)]
    #[ts(skip)]
    pub format: Option<CellFormatProjection>,
    #[serde(default, skip)]
    #[ts(skip)]
    pub style: Option<CellStyleProjection>,
    #[serde(default)]
    #[ts(skip)]
    pub display_format: Option<CellFormatProjection>,
}

impl SheetCellChange {
    pub fn new(sheet_index: usize, row: usize, col: usize, value: DomainCellValue) -> Self {
        Self {
            sheet_index,
            row,
            col,
            value: value.into(),
            display: None,
            format: None,
            style: None,
            display_format: None,
        }
    }

    pub fn with_display_projection(
        mut self,
        display: String,
        format: Option<CellFormatProjection>,
        style: Option<CellStyleProjection>,
    ) -> Self {
        self.display = Some(display);
        self.format = format.clone();
        self.style = style;
        self.display_format = format;
        self
    }
}

impl Serialize for SheetCellChange {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut state = serializer.serialize_struct("SheetCellChange", 4)?;
        state.serialize_field("sheetIndex", &self.sheet_index)?;
        state.serialize_field("row", &self.row)?;
        state.serialize_field("col", &self.col)?;
        state.serialize_field(
            "value",
            &CellValueProjection::new(self.value.as_domain(), self.display_format.clone()),
        )?;
        state.end()
    }
}
