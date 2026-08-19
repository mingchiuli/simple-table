use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};

use simple_table_protocol::{SHEET_REGION_TILE_COLUMNS, SHEET_REGION_TILE_ROWS};

use super::{
    CellPresentation, CellView, EditorMutationView, EditorPatchView, MergeRangeView,
    SheetRegionBoundsView, SheetRegionView, cell_presentation,
};

const MAX_TILES_PER_SHEET: usize = 8;
const MAX_TILES_GLOBAL: usize = 24;
const MAX_CACHE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct DocumentRevision {
    pub document_id: u64,
    pub revision: u64,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
pub struct RegionTileKey {
    pub sheet_index: usize,
    pub row_start: usize,
    pub col_start: usize,
}

impl RegionTileKey {
    pub fn from_bounds(bounds: SheetRegionBoundsView) -> Self {
        Self {
            sheet_index: bounds.sheet_index,
            row_start: bounds.row_start,
            col_start: bounds.col_start,
        }
    }
}

#[derive(Clone, Debug)]
struct CachedRegionTile {
    bounds: SheetRegionBoundsView,
    fragments: Vec<SheetRegionView>,
    wire_bytes: usize,
}

#[derive(Clone, Debug, Default)]
pub struct RegionProjection {
    pub cells: HashMap<(usize, usize), CellPresentation>,
    pub merges: Vec<MergeRangeView>,
}

#[derive(Clone, Debug, Default)]
pub struct RegionCache {
    identity: Option<DocumentRevision>,
    tiles: HashMap<RegionTileKey, CachedRegionTile>,
    lru: RefCell<VecDeque<RegionTileKey>>,
    visible: HashSet<RegionTileKey>,
    wire_bytes: usize,
}

impl RegionCache {
    pub fn new(identity: DocumentRevision) -> Self {
        Self {
            identity: Some(identity),
            ..Self::default()
        }
    }

    pub fn contains(&self, identity: DocumentRevision, bounds: SheetRegionBoundsView) -> bool {
        self.identity == Some(identity)
            && self
                .tiles
                .get(&RegionTileKey::from_bounds(bounds))
                .is_some_and(|tile| tile.bounds == bounds)
    }

    pub fn set_visible(&mut self, keys: impl IntoIterator<Item = RegionTileKey>) {
        self.visible.clear();
        self.visible.extend(keys);
        self.evict();
    }

    pub fn insert_region(
        &mut self,
        logical_bounds: SheetRegionBoundsView,
        fragments: Vec<SheetRegionView>,
    ) -> bool {
        let Some(first) = fragments.first() else {
            return false;
        };
        let identity = DocumentRevision {
            document_id: first.document_id,
            revision: first.revision,
        };
        if self.identity.is_some_and(|current| current != identity) {
            return false;
        }
        self.identity = Some(identity);
        if fragments.iter().any(|fragment| {
            fragment.document_id != identity.document_id
                || fragment.revision != identity.revision
                || fragment.region.sheet_index != logical_bounds.sheet_index
                || !region_contains(logical_bounds, fragment.region)
                || fragment
                    .cells
                    .iter()
                    .any(|cell| !contains_cell(fragment.region, cell))
                || fragment
                    .merge_anchor_cells
                    .iter()
                    .any(|cell| cell.sheet_index != logical_bounds.sheet_index)
        }) {
            return false;
        }

        let key = RegionTileKey::from_bounds(logical_bounds);
        let wire_bytes = fragments.iter().map(|fragment| fragment.wire_bytes).sum();
        if let Some(replaced) = self.tiles.insert(
            key,
            CachedRegionTile {
                bounds: logical_bounds,
                fragments,
                wire_bytes,
            },
        ) {
            self.wire_bytes = self.wire_bytes.saturating_sub(replaced.wire_bytes);
        }
        self.wire_bytes = self.wire_bytes.saturating_add(wire_bytes);
        self.touch(key);
        self.evict();
        true
    }

    pub fn projection(
        &self,
        sheet_index: usize,
        bounds: Option<SheetRegionBoundsView>,
    ) -> RegionProjection {
        let mut projection = RegionProjection::default();
        let mut merges = Vec::new();
        for (key, tile) in self.tiles.iter().filter(|(_, tile)| {
            tile.bounds.sheet_index == sheet_index
                && bounds.is_none_or(|bounds| regions_intersect(tile.bounds, bounds))
        }) {
            self.touch(*key);
            for fragment in &tile.fragments {
                for cell in fragment
                    .cells
                    .iter()
                    .filter(|cell| bounds.is_none_or(|bounds| contains_cell(bounds, cell)))
                    .chain(fragment.merge_anchor_cells.iter())
                {
                    projection
                        .cells
                        .insert((cell.row, cell.col), cell_presentation(&cell.value));
                }
                merges.extend(fragment.metadata.merges.iter().copied().filter(|merge| {
                    bounds.is_none_or(|bounds| {
                        merge.intersects(
                            bounds.row_start,
                            bounds.row_end,
                            bounds.col_start,
                            bounds.col_end,
                        )
                    })
                }));
            }
        }
        merges.sort_unstable_by_key(|merge| {
            (
                merge.start_row,
                merge.start_col,
                merge.end_row,
                merge.end_col,
            )
        });
        merges.dedup();
        let mut accepted = Vec::with_capacity(merges.len());
        for merge in merges {
            if accepted
                .iter()
                .any(|existing: &MergeRangeView| existing.overlaps(merge))
            {
                continue;
            }
            accepted.push(merge);
        }
        projection.merges = accepted;
        projection
    }

    pub fn merge_range_at(
        &self,
        sheet_index: usize,
        row: usize,
        col: usize,
    ) -> Option<MergeRangeView> {
        self.projection(sheet_index, None)
            .merges
            .into_iter()
            .find(|merge| merge.contains(row, col))
    }

    pub fn clear(&mut self) {
        self.tiles.clear();
        self.lru.get_mut().clear();
        self.visible.clear();
        self.wire_bytes = 0;
    }

    pub fn clear_sheet(&mut self, sheet_index: usize) {
        let keys = self
            .tiles
            .keys()
            .copied()
            .filter(|key| key.sheet_index == sheet_index)
            .collect::<Vec<_>>();
        for key in keys {
            self.remove(key);
        }
    }

    pub fn apply_mutation(&mut self, mutation: &EditorMutationView) {
        let Some(identity) = self.identity else {
            return;
        };
        if identity.document_id != mutation.document_id || mutation.revision < identity.revision {
            self.clear();
            self.identity = Some(DocumentRevision {
                document_id: mutation.document_id,
                revision: mutation.revision,
            });
            return;
        }

        for patch in &mutation.patches {
            match patch {
                EditorPatchView::Cells { changes } => {
                    for change in changes {
                        self.apply_cell_change(change);
                    }
                }
                EditorPatchView::Layout { .. } => {}
                EditorPatchView::SheetInvalidated { patch }
                | EditorPatchView::RowInserted { patch }
                | EditorPatchView::RowDeleted { patch }
                | EditorPatchView::ColumnInserted { patch }
                | EditorPatchView::ColumnDeleted { patch } => self.clear_sheet(patch.sheet_index),
                EditorPatchView::ImageUpserted { .. } | EditorPatchView::ImageDeleted { .. } => {}
                EditorPatchView::SheetInserted { .. }
                | EditorPatchView::SheetDeleted { .. }
                | EditorPatchView::SheetsReplaced { .. }
                | EditorPatchView::ResyncRequired { .. } => self.clear(),
            }
        }
        self.identity = Some(DocumentRevision {
            document_id: mutation.document_id,
            revision: mutation.revision,
        });
    }

    fn apply_cell_change(&mut self, change: &CellView) {
        for tile in self.tiles.values_mut().filter(|tile| {
            tile.bounds.sheet_index == change.sheet_index
                && (contains_cell(tile.bounds, change)
                    || tile.fragments.iter().any(|fragment| {
                        fragment.merge_anchor_cells.iter().any(|cell| {
                            cell.sheet_index == change.sheet_index
                                && cell.row == change.row
                                && cell.col == change.col
                        })
                    }))
        }) {
            for fragment in &mut tile.fragments {
                if let Some(cell) = fragment.merge_anchor_cells.iter_mut().find(|cell| {
                    cell.sheet_index == change.sheet_index
                        && cell.row == change.row
                        && cell.col == change.col
                }) {
                    *cell = change.clone();
                }
                if let Some(cell) = fragment.cells.iter_mut().find(|cell| {
                    cell.sheet_index == change.sheet_index
                        && cell.row == change.row
                        && cell.col == change.col
                }) {
                    *cell = change.clone();
                } else if contains_cell(fragment.region, change) {
                    fragment.cells.push(change.clone());
                }
            }
        }
    }

    fn touch(&self, key: RegionTileKey) {
        let mut lru = self.lru.borrow_mut();
        lru.retain(|candidate| *candidate != key);
        lru.push_back(key);
    }

    fn evict(&mut self) {
        loop {
            let over_global = self.tiles.len() > MAX_TILES_GLOBAL;
            let over_bytes = self.wire_bytes > MAX_CACHE_BYTES;
            let crowded_sheet = self.tiles.keys().find_map(|key| {
                (self
                    .tiles
                    .keys()
                    .filter(|candidate| candidate.sheet_index == key.sheet_index)
                    .count()
                    > MAX_TILES_PER_SHEET)
                    .then_some(key.sheet_index)
            });
            if !over_global && !over_bytes && crowded_sheet.is_none() {
                break;
            }
            let Some(position) = self.lru.get_mut().iter().position(|key| {
                !self.visible.contains(key)
                    && crowded_sheet.is_none_or(|sheet| key.sheet_index == sheet)
            }) else {
                break;
            };
            let key = self.lru.get_mut()[position];
            self.remove(key);
        }
    }

    fn remove(&mut self, key: RegionTileKey) {
        if let Some(tile) = self.tiles.remove(&key) {
            self.wire_bytes = self.wire_bytes.saturating_sub(tile.wire_bytes);
        }
        self.lru.get_mut().retain(|candidate| *candidate != key);
        self.visible.remove(&key);
    }
}

pub fn tile_bounds(
    sheet_index: usize,
    row: usize,
    col: usize,
    row_count: usize,
    column_count: usize,
) -> SheetRegionBoundsView {
    let row_start = row / SHEET_REGION_TILE_ROWS * SHEET_REGION_TILE_ROWS;
    let col_start = col / SHEET_REGION_TILE_COLUMNS * SHEET_REGION_TILE_COLUMNS;
    SheetRegionBoundsView {
        sheet_index,
        row_start,
        row_end: row_start
            .saturating_add(SHEET_REGION_TILE_ROWS)
            .min(row_count),
        col_start,
        col_end: col_start
            .saturating_add(SHEET_REGION_TILE_COLUMNS)
            .min(column_count),
    }
}

pub fn tiles_for_region(
    bounds: SheetRegionBoundsView,
    row_count: usize,
    column_count: usize,
) -> Vec<SheetRegionBoundsView> {
    if bounds.row_start >= row_count || bounds.col_start >= column_count {
        return Vec::new();
    }
    let mut tiles = Vec::new();
    let mut row = bounds.row_start / SHEET_REGION_TILE_ROWS * SHEET_REGION_TILE_ROWS;
    while row < bounds.row_end.min(row_count) {
        let mut col = bounds.col_start / SHEET_REGION_TILE_COLUMNS * SHEET_REGION_TILE_COLUMNS;
        while col < bounds.col_end.min(column_count) {
            tiles.push(tile_bounds(
                bounds.sheet_index,
                row,
                col,
                row_count,
                column_count,
            ));
            col = col.saturating_add(SHEET_REGION_TILE_COLUMNS);
        }
        row = row.saturating_add(SHEET_REGION_TILE_ROWS);
    }
    tiles
}

fn contains_cell(bounds: SheetRegionBoundsView, cell: &CellView) -> bool {
    cell.sheet_index == bounds.sheet_index
        && cell.row >= bounds.row_start
        && cell.row < bounds.row_end
        && cell.col >= bounds.col_start
        && cell.col < bounds.col_end
}

fn regions_intersect(first: SheetRegionBoundsView, second: SheetRegionBoundsView) -> bool {
    first.row_start < second.row_end
        && first.row_end > second.row_start
        && first.col_start < second.col_end
        && first.col_end > second.col_start
}

fn region_contains(outer: SheetRegionBoundsView, inner: SheetRegionBoundsView) -> bool {
    inner.row_start >= outer.row_start
        && inner.row_end <= outer.row_end
        && inner.col_start >= outer.col_start
        && inner.col_end <= outer.col_end
        && inner.row_start < inner.row_end
        && inner.col_start < inner.col_end
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::model::SheetRegionMetadataView;

    fn region(row_start: usize, value: &str) -> SheetRegionView {
        SheetRegionView {
            document_id: 9,
            revision: 3,
            region: SheetRegionBoundsView {
                sheet_index: 0,
                row_start,
                row_end: row_start + SHEET_REGION_TILE_ROWS,
                col_start: 0,
                col_end: SHEET_REGION_TILE_COLUMNS,
            },
            cells: vec![CellView {
                sheet_index: 0,
                row: row_start,
                col: 0,
                value: json!({"raw": value}),
            }],
            merge_anchor_cells: Vec::new(),
            metadata: SheetRegionMetadataView::default(),
            wire_bytes: 128,
        }
    }

    #[test]
    fn tile_projection_combines_cached_regions() {
        let mut cache = RegionCache::new(DocumentRevision {
            document_id: 9,
            revision: 3,
        });
        let first = region(0, "first");
        let second = region(SHEET_REGION_TILE_ROWS, "second");
        assert!(cache.insert_region(first.region, vec![first]));
        assert!(cache.insert_region(second.region, vec![second]));

        let projection = cache.projection(0, None);
        assert_eq!(projection.cells[&(0, 0)].display_text, "first");
        assert_eq!(
            projection.cells[&(SHEET_REGION_TILE_ROWS, 0)].display_text,
            "second"
        );
    }

    #[test]
    fn stale_region_is_rejected() {
        let mut cache = RegionCache::new(DocumentRevision {
            document_id: 9,
            revision: 4,
        });
        let stale = region(0, "stale");

        assert!(!cache.insert_region(stale.region, vec![stale]));
    }

    #[test]
    fn visible_tiles_are_pinned_during_eviction() {
        let mut cache = RegionCache::new(DocumentRevision {
            document_id: 9,
            revision: 3,
        });
        let pinned = RegionTileKey {
            sheet_index: 0,
            row_start: 0,
            col_start: 0,
        };
        cache.set_visible([pinned]);
        for index in 0..=MAX_TILES_PER_SHEET {
            let item = region(index * SHEET_REGION_TILE_ROWS, "value");
            cache.insert_region(item.region, vec![item]);
        }

        assert!(cache.tiles.contains_key(&pinned));
        assert_eq!(cache.tiles.len(), MAX_TILES_PER_SHEET);
    }

    #[test]
    fn projection_refreshes_the_lru_position() {
        let mut cache = RegionCache::new(DocumentRevision {
            document_id: 9,
            revision: 3,
        });
        for index in 0..MAX_TILES_PER_SHEET {
            let item = region(index * SHEET_REGION_TILE_ROWS, "value");
            cache.insert_region(item.region, vec![item]);
        }
        let first_bounds = region(0, "first").region;
        cache.projection(0, Some(first_bounds));
        let additional = region(MAX_TILES_PER_SHEET * SHEET_REGION_TILE_ROWS, "new");
        cache.insert_region(additional.region, vec![additional]);

        assert!(
            cache
                .tiles
                .contains_key(&RegionTileKey::from_bounds(first_bounds))
        );
        assert!(!cache.tiles.contains_key(&RegionTileKey {
            sheet_index: 0,
            row_start: SHEET_REGION_TILE_ROWS,
            col_start: 0,
        }));
    }

    #[test]
    fn projection_keeps_an_external_merge_anchor() {
        let mut cache = RegionCache::new(DocumentRevision {
            document_id: 9,
            revision: 3,
        });
        let mut item = region(0, "unused");
        item.region = SheetRegionBoundsView {
            sheet_index: 0,
            row_start: 8,
            row_end: 12,
            col_start: 1,
            col_end: 3,
        };
        item.cells.clear();
        item.merge_anchor_cells.push(CellView {
            sheet_index: 0,
            row: 2,
            col: 1,
            value: json!({"raw": "Merged"}),
        });
        item.metadata.merges.push(MergeRangeView {
            start_row: 2,
            start_col: 1,
            end_row: 10,
            end_col: 2,
        });
        assert!(cache.insert_region(item.region, vec![item]));

        let projection = cache.projection(
            0,
            Some(SheetRegionBoundsView {
                sheet_index: 0,
                row_start: 8,
                row_end: 12,
                col_start: 1,
                col_end: 3,
            }),
        );

        assert_eq!(projection.cells[&(2, 1)].display_text, "Merged");
        assert_eq!(projection.merges.len(), 1);
    }
}
