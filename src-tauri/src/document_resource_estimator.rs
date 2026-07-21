use crate::document_data::{
    CellFormat, CellStyle, DocumentData, DocumentSheet, Drawing, FreezePane, Hyperlink, MergeRange,
    RichMetadata,
};
use crate::domain::CellValue;

const MAP_ENTRY_OVERHEAD_BYTES: usize = 32;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct MetadataTextUsage {
    pub total_bytes: usize,
    pub maximum_string_bytes: usize,
}

impl MetadataTextUsage {
    fn include(&mut self, value: &str) {
        self.total_bytes = self.total_bytes.saturating_add(value.len());
        self.maximum_string_bytes = self.maximum_string_bytes.max(value.len());
    }

    fn include_optional(&mut self, value: Option<&String>) {
        if let Some(value) = value {
            self.include(value);
        }
    }

    fn merge(&mut self, other: Self) {
        self.total_bytes = self.total_bytes.saturating_add(other.total_bytes);
        self.maximum_string_bytes = self.maximum_string_bytes.max(other.maximum_string_bytes);
    }
}

pub(crate) fn document_metadata_text_usage(file_data: &DocumentData) -> MetadataTextUsage {
    let mut usage = MetadataTextUsage::default();
    usage.include(&file_data.path);
    usage.include(&file_data.file_name);
    for sheet in &file_data.sheets {
        usage.merge(sheet_metadata_text_usage(sheet));
    }
    usage
}

pub(crate) fn sheet_metadata_text_usage(sheet: &DocumentSheet) -> MetadataTextUsage {
    let mut usage = MetadataTextUsage::default();
    usage.include(&sheet.name);

    for (key, format) in &sheet.rich.cell_formats {
        usage.include(key);
        usage.include_optional(format.number_format.as_ref());
        usage.include_optional(format.style_id.as_ref());
    }
    for (key, style) in &sheet.rich.cell_styles {
        usage.include(key);
        usage.include_optional(style.font_color.as_ref());
        usage.include_optional(style.background_color.as_ref());
        usage.include_optional(style.horizontal_align.as_ref());
        usage.include_optional(style.vertical_align.as_ref());
        usage.include_optional(style.number_format.as_ref());
    }
    if let Some(pane) = &sheet.rich.freeze_pane {
        usage.include(&pane.top_left_cell);
        usage.include(&pane.active_pane);
        usage.include(&pane.state);
    }
    for (key, hyperlink) in &sheet.rich.hyperlinks {
        usage.include(key);
        usage.include(&hyperlink.url);
        usage.include_optional(hyperlink.tooltip.as_ref());
    }
    usage
}

pub(crate) fn estimate_document_metadata_bytes(file_data: &DocumentData) -> usize {
    file_data
        .path
        .len()
        .saturating_add(file_data.file_name.len())
        .saturating_add(
            file_data
                .sheets
                .iter()
                .map(|sheet| {
                    sheet
                        .name
                        .len()
                        .saturating_add(sheet.merges.len() * std::mem::size_of::<MergeRange>())
                        .saturating_add(sheet.column_widths.as_ref().map_or(0, |values| {
                            values.len()
                                * (std::mem::size_of::<(usize, u32)>() + MAP_ENTRY_OVERHEAD_BYTES)
                        }))
                        .saturating_add(sheet.row_heights.as_ref().map_or(0, |values| {
                            values.len()
                                * (std::mem::size_of::<(usize, u32)>() + MAP_ENTRY_OVERHEAD_BYTES)
                        }))
                        .saturating_add(estimate_rich_metadata_bytes(&sheet.rich))
                })
                .sum::<usize>(),
        )
}

pub(crate) fn estimate_sheet_data_bytes(sheet: &DocumentSheet) -> usize {
    std::mem::size_of::<DocumentSheet>()
        .saturating_add(sheet.name.len())
        .saturating_add(sheet.rows.len() * std::mem::size_of::<Vec<CellValue>>())
        .saturating_add(
            sheet
                .rows
                .iter()
                .flatten()
                .map(estimate_cell_value_bytes)
                .sum::<usize>(),
        )
        .saturating_add(sheet.merges.len() * std::mem::size_of::<MergeRange>())
        .saturating_add(sheet.column_widths.as_ref().map_or(0, |widths| {
            widths.len() * (std::mem::size_of::<(usize, u32)>() + MAP_ENTRY_OVERHEAD_BYTES)
        }))
        .saturating_add(sheet.row_heights.as_ref().map_or(0, |heights| {
            heights.len() * (std::mem::size_of::<(usize, u32)>() + MAP_ENTRY_OVERHEAD_BYTES)
        }))
        .saturating_add(estimate_rich_metadata_bytes(&sheet.rich))
}

pub(crate) fn estimate_cell_value_bytes(cell: &CellValue) -> usize {
    match cell {
        CellValue::Null | CellValue::Boolean(_) => std::mem::size_of::<CellValue>(),
        CellValue::String(value) => std::mem::size_of::<CellValue>() + value.len(),
        CellValue::Number(value) => std::mem::size_of::<CellValue>() + value.to_string().len(),
        CellValue::Formula {
            formula,
            cached_value,
            error,
        } => std::mem::size_of::<CellValue>()
            .saturating_add(formula.len())
            .saturating_add(estimate_cell_value_bytes(cached_value))
            .saturating_add(error.as_ref().map_or(0, String::len)),
    }
}

pub(crate) fn estimate_rich_metadata_bytes(rich: &RichMetadata) -> usize {
    std::mem::size_of::<RichMetadata>()
        .saturating_add(
            rich.cell_formats
                .iter()
                .map(|(cell, format)| {
                    MAP_ENTRY_OVERHEAD_BYTES + cell.len() + estimate_cell_format_bytes(format)
                })
                .sum::<usize>(),
        )
        .saturating_add(
            rich.cell_styles
                .iter()
                .map(|(cell, style)| {
                    MAP_ENTRY_OVERHEAD_BYTES + cell.len() + estimate_cell_style_bytes(style)
                })
                .sum::<usize>(),
        )
        .saturating_add(rich.hidden_rows.len() * std::mem::size_of::<usize>())
        .saturating_add(rich.hidden_columns.len() * std::mem::size_of::<usize>())
        .saturating_add(
            rich.freeze_pane
                .as_ref()
                .map_or(0, estimate_freeze_pane_bytes),
        )
        .saturating_add(
            rich.hyperlinks
                .iter()
                .map(|(cell, hyperlink)| {
                    MAP_ENTRY_OVERHEAD_BYTES + cell.len() + estimate_hyperlink_bytes(hyperlink)
                })
                .sum::<usize>(),
        )
        .saturating_add(rich.drawings.len() * std::mem::size_of::<Drawing>())
}

pub(crate) fn estimate_cell_format_bytes(format: &CellFormat) -> usize {
    std::mem::size_of::<CellFormat>()
        + format.number_format.as_ref().map_or(0, String::len)
        + format.style_id.as_ref().map_or(0, String::len)
}

pub(crate) fn estimate_cell_style_bytes(style: &CellStyle) -> usize {
    std::mem::size_of::<CellStyle>()
        + style.font_color.as_ref().map_or(0, String::len)
        + style.background_color.as_ref().map_or(0, String::len)
        + style.horizontal_align.as_ref().map_or(0, String::len)
        + style.vertical_align.as_ref().map_or(0, String::len)
        + style.number_format.as_ref().map_or(0, String::len)
}

pub(crate) fn estimate_freeze_pane_bytes(freeze_pane: &FreezePane) -> usize {
    std::mem::size_of::<FreezePane>()
        + freeze_pane.top_left_cell.len()
        + freeze_pane.active_pane.len()
        + freeze_pane.state.len()
}

pub(crate) fn estimate_hyperlink_bytes(hyperlink: &Hyperlink) -> usize {
    std::mem::size_of::<Hyperlink>()
        + hyperlink.url.len()
        + hyperlink.tooltip.as_ref().map_or(0, String::len)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_usage_counts_dynamic_values_instead_of_only_entries() {
        let mut short = DocumentSheet::default();
        short.rich.hyperlinks.insert(
            "A1".into(),
            Hyperlink {
                url: "x".into(),
                tooltip: None,
                location: false,
            },
        );
        let mut long = short.clone();
        long.rich.hyperlinks.get_mut("A1").unwrap().url = "x".repeat(8_192);

        assert!(
            sheet_metadata_text_usage(&long).total_bytes
                > sheet_metadata_text_usage(&short).total_bytes + 8_000
        );
        assert!(
            estimate_rich_metadata_bytes(&long.rich) > estimate_rich_metadata_bytes(&short.rich)
        );
    }
}
