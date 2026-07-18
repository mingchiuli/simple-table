use crate::document_data::{DocumentData, DocumentSheet};
use std::collections::HashMap;

use crate::domain::cell_key::parse_cell_key;
use crate::types::{MergeRange, SheetRegion, SheetRegionMetadata};

const TILE_ROWS: usize = 128;
const TILE_COLUMNS: usize = 32;

#[derive(Default)]
pub(crate) struct RegionMetadataIndex {
    sheets: Vec<SheetMetadataIndex>,
}

#[derive(Default)]
struct SheetMetadataIndex {
    merges: MergeIntervalIndex,
    format_keys: CellKeyBuckets,
    style_keys: CellKeyBuckets,
}

#[derive(Default)]
struct CellKeyBuckets {
    buckets: HashMap<(usize, usize), Vec<IndexedCellKey>>,
}

struct IndexedCellKey {
    row: usize,
    col: usize,
    key: String,
}

#[derive(Default)]
struct MergeIntervalIndex {
    root: Option<Box<MergeIntervalNode>>,
    entry_count: usize,
}

struct MergeIntervalNode {
    center: usize,
    by_start: Vec<MergeRange>,
    by_end: Vec<MergeRange>,
    left: Option<Box<MergeIntervalNode>>,
    right: Option<Box<MergeIntervalNode>>,
}

impl RegionMetadataIndex {
    pub(crate) fn from_file_data(file_data: &DocumentData) -> Self {
        Self {
            sheets: file_data
                .sheets
                .iter()
                .map(SheetMetadataIndex::from_sheet)
                .collect(),
        }
    }

    pub(crate) fn rebuild(&mut self, file_data: &DocumentData) {
        *self = Self::from_file_data(file_data);
    }

    pub(crate) fn project(
        &self,
        file_data: &DocumentData,
        region: &SheetRegion,
    ) -> SheetRegionMetadata {
        if region.row_start >= region.row_end || region.col_start >= region.col_end {
            return SheetRegionMetadata {
                merges: Vec::new(),
                cell_formats: HashMap::new(),
                cell_styles: HashMap::new(),
            };
        }
        let Some(sheet) = file_data.sheets.get(region.sheet_index) else {
            return SheetRegionMetadata {
                merges: Vec::new(),
                cell_formats: HashMap::new(),
                cell_styles: HashMap::new(),
            };
        };
        let Some(index) = self.sheets.get(region.sheet_index) else {
            return SheetMetadataIndex::from_sheet(sheet).project(sheet, region);
        };
        index.project(sheet, region)
    }

    pub(crate) fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self
                .sheets
                .iter()
                .map(SheetMetadataIndex::estimated_bytes)
                .sum::<usize>()
    }
}

impl SheetMetadataIndex {
    fn from_sheet(sheet: &DocumentSheet) -> Self {
        Self {
            merges: MergeIntervalIndex::new(&sheet.merges),
            format_keys: CellKeyBuckets::new(sheet.rich.cell_formats.keys()),
            style_keys: CellKeyBuckets::new(sheet.rich.cell_styles.keys()),
        }
    }

    fn project(&self, sheet: &DocumentSheet, region: &SheetRegion) -> SheetRegionMetadata {
        SheetRegionMetadata {
            merges: self.merges.query(region),
            cell_formats: self
                .format_keys
                .keys_in_region(region)
                .filter_map(|key| {
                    sheet
                        .rich
                        .cell_formats
                        .get(key)
                        .cloned()
                        .map(|value| (key.to_string(), value))
                })
                .collect(),
            cell_styles: self
                .style_keys
                .keys_in_region(region)
                .filter_map(|key| {
                    sheet
                        .rich
                        .cell_styles
                        .get(key)
                        .cloned()
                        .map(|value| (key.to_string(), value))
                })
                .collect(),
        }
    }

    fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.merges.estimated_bytes()
            + self.format_keys.estimated_bytes()
            + self.style_keys.estimated_bytes()
    }
}

impl CellKeyBuckets {
    fn new<'a>(keys: impl Iterator<Item = &'a String>) -> Self {
        let mut buckets: HashMap<(usize, usize), Vec<IndexedCellKey>> = HashMap::new();
        for key in keys {
            let Some((row, col)) = parse_cell_key(key) else {
                continue;
            };
            buckets
                .entry((row / TILE_ROWS, col / TILE_COLUMNS))
                .or_default()
                .push(IndexedCellKey {
                    row,
                    col,
                    key: key.clone(),
                });
        }
        Self { buckets }
    }

    fn keys_in_region<'a>(&'a self, region: &'a SheetRegion) -> impl Iterator<Item = &'a str> + 'a {
        let row_buckets = bucket_range(region.row_start, region.row_end, TILE_ROWS);
        let col_buckets = bucket_range(region.col_start, region.col_end, TILE_COLUMNS);
        row_buckets.flat_map(move |row_bucket| {
            col_buckets.clone().flat_map(move |col_bucket| {
                self.buckets
                    .get(&(row_bucket, col_bucket))
                    .into_iter()
                    .flatten()
                    .filter(move |entry| {
                        entry.row >= region.row_start
                            && entry.row < region.row_end
                            && entry.col >= region.col_start
                            && entry.col < region.col_end
                    })
                    .map(|entry| entry.key.as_str())
            })
        })
    }

    fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.buckets.capacity() * 48
            + self
                .buckets
                .values()
                .flat_map(|entries| entries.iter())
                .map(|entry| std::mem::size_of::<IndexedCellKey>() + entry.key.capacity())
                .sum::<usize>()
    }
}

impl MergeIntervalIndex {
    fn new(merges: &[MergeRange]) -> Self {
        Self {
            root: MergeIntervalNode::build(merges.to_vec()),
            entry_count: merges.len(),
        }
    }

    fn query(&self, region: &SheetRegion) -> Vec<MergeRange> {
        let mut merges = Vec::new();
        if let Some(root) = &self.root {
            root.query(region, &mut merges);
        }
        merges
    }

    fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Self>() + self.entry_count * 2 * std::mem::size_of::<MergeRange>()
    }
}

impl MergeIntervalNode {
    fn build(merges: Vec<MergeRange>) -> Option<Box<Self>> {
        if merges.is_empty() {
            return None;
        }
        let mut midpoints: Vec<_> = merges
            .iter()
            .map(|merge| (merge.start_row as usize).saturating_add(merge.end_row as usize) / 2)
            .collect();
        midpoints.sort_unstable();
        let center = midpoints[midpoints.len() / 2];
        let mut left = Vec::new();
        let mut right = Vec::new();
        let mut overlaps = Vec::new();
        for merge in merges {
            if (merge.end_row as usize) < center {
                left.push(merge);
            } else if merge.start_row as usize > center {
                right.push(merge);
            } else {
                overlaps.push(merge);
            }
        }
        let mut by_start = overlaps.clone();
        by_start.sort_by_key(|merge| merge.start_row);
        overlaps.sort_by(|left, right| right.end_row.cmp(&left.end_row));
        Some(Box::new(Self {
            center,
            by_start,
            by_end: overlaps,
            left: Self::build(left),
            right: Self::build(right),
        }))
    }

    fn query(&self, region: &SheetRegion, output: &mut Vec<MergeRange>) {
        if region.row_end <= self.center {
            for merge in self
                .by_start
                .iter()
                .take_while(|merge| (merge.start_row as usize) < region.row_end)
            {
                push_if_columns_overlap(merge, region, output);
            }
            if let Some(left) = &self.left {
                left.query(region, output);
            }
            return;
        }
        if region.row_start > self.center {
            for merge in self
                .by_end
                .iter()
                .take_while(|merge| merge.end_row as usize >= region.row_start)
            {
                push_if_columns_overlap(merge, region, output);
            }
            if let Some(right) = &self.right {
                right.query(region, output);
            }
            return;
        }

        for merge in &self.by_start {
            push_if_columns_overlap(merge, region, output);
        }
        if let Some(left) = &self.left {
            left.query(region, output);
        }
        if let Some(right) = &self.right {
            right.query(region, output);
        }
    }
}

fn push_if_columns_overlap(merge: &MergeRange, region: &SheetRegion, output: &mut Vec<MergeRange>) {
    if (merge.start_col as usize) < region.col_end && merge.end_col as usize >= region.col_start {
        output.push(merge.clone());
    }
}

fn bucket_range(start: usize, end: usize, size: usize) -> std::ops::RangeInclusive<usize> {
    let first = start / size;
    let last = end.saturating_sub(1).max(start) / size;
    first..=last
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{CellFormatProjection, CellStyleProjection, ReadOnlyRichProjection};

    #[test]
    fn projects_only_metadata_intersecting_the_requested_region() {
        let sheet = DocumentSheet {
            merges: vec![
                merge(100, 40, 140, 42),
                merge(100, 2, 140, 3),
                merge(300, 40, 320, 42),
            ],
            rich: ReadOnlyRichProjection {
                cell_formats: HashMap::from([
                    ("A1".to_string(), format("outside")),
                    ("AG129".to_string(), format("inside")),
                ]),
                cell_styles: HashMap::from([
                    ("A1".to_string(), style("outside")),
                    ("BF200".to_string(), style("inside")),
                ]),
                ..Default::default()
            },
            ..Default::default()
        };
        let file_data = DocumentData {
            path: String::new(),
            file_name: "metadata.xlsx".to_string(),
            sheets: vec![sheet],
        };
        let index = RegionMetadataIndex::from_file_data(&file_data);
        let metadata = index.project(
            &file_data,
            &SheetRegion {
                sheet_index: 0,
                row_start: 128,
                row_end: 256,
                col_start: 32,
                col_end: 64,
            },
        );

        assert_eq!(metadata.merges, vec![merge(100, 40, 140, 42)]);
        assert_eq!(
            metadata.cell_formats.keys().collect::<Vec<_>>(),
            vec!["AG129"]
        );
        assert_eq!(
            metadata.cell_styles.keys().collect::<Vec<_>>(),
            vec!["BF200"]
        );
    }

    #[test]
    fn rebuild_replaces_stale_bucket_entries() {
        let mut file_data = DocumentData {
            path: String::new(),
            file_name: "metadata.xlsx".to_string(),
            sheets: vec![DocumentSheet {
                rich: ReadOnlyRichProjection {
                    cell_formats: HashMap::from([("A1".to_string(), format("old"))]),
                    ..Default::default()
                },
                ..Default::default()
            }],
        };
        let mut index = RegionMetadataIndex::from_file_data(&file_data);
        file_data.sheets[0].rich.cell_formats.clear();
        file_data.sheets[0]
            .rich
            .cell_formats
            .insert("AG129".to_string(), format("new"));
        index.rebuild(&file_data);

        let metadata = index.project(
            &file_data,
            &SheetRegion {
                sheet_index: 0,
                row_start: 128,
                row_end: 256,
                col_start: 32,
                col_end: 64,
            },
        );

        assert!(metadata.cell_formats.contains_key("AG129"));
        assert!(!metadata.cell_formats.contains_key("A1"));
    }

    fn merge(start_row: u32, start_col: u16, end_row: u32, end_col: u16) -> MergeRange {
        MergeRange {
            start_row,
            start_col,
            end_row,
            end_col,
        }
    }

    fn format(value: &str) -> CellFormatProjection {
        CellFormatProjection {
            number_format: Some(value.to_string()),
            style_id: None,
        }
    }

    fn style(value: &str) -> CellStyleProjection {
        CellStyleProjection {
            background_color: Some(value.to_string()),
            ..Default::default()
        }
    }
}
