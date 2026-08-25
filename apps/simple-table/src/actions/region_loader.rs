use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::time::{Duration, Instant};

use dioxus::prelude::{ReadableExt, WritableExt, spawn};
use futures::channel::oneshot;

use crate::model::region_cache::tiles_for_region;
use crate::model::{
    DocumentRevision, EditorStore, RegionTileKey, SheetExtentView, SheetRegionBoundsView,
    SheetRegionView, SheetRowsRegionView,
};
use crate::ports::editor::EditorPort;
use crate::protocol::{AppErrorDto, EditorReply, EditorRequest};
use simple_table_protocol::SHEET_REGION_TILE_COLUMNS;

const MAX_QUEUED_REGIONS: usize = 1_024;
const MAX_ROWS_PER_REGION_REQUEST: usize = 1_024;
const MAX_SPLIT_REQUESTS: usize = 64;
const SPLIT_DEADLINE: Duration = Duration::from_secs(10);

type Waiter = oneshot::Sender<Result<(), AppErrorDto>>;

#[derive(Clone)]
pub struct RegionLoader(Rc<RegionLoaderInner>);

struct RegionLoaderInner {
    editor: Rc<dyn EditorPort>,
    state: RefCell<LoaderState>,
}

#[derive(Default)]
struct LoaderState {
    running: bool,
    queue: VecDeque<RegionJob>,
    scheduled: HashSet<RegionJobKey>,
    required_waiters: HashMap<RegionJobKey, Vec<Waiter>>,
    viewport: HashSet<RegionJobKey>,
}

#[derive(Clone, Copy, Debug, Hash, PartialEq, Eq)]
struct RegionJobKey {
    identity: DocumentRevision,
    bounds: SheetRegionBoundsView,
}

#[derive(Clone, Copy)]
struct RegionJob {
    key: RegionJobKey,
    store: EditorStore,
}

impl RegionLoader {
    pub fn new(editor: Rc<dyn EditorPort>) -> Self {
        Self(Rc::new(RegionLoaderInner {
            editor,
            state: RefCell::new(LoaderState::default()),
        }))
    }

    pub fn schedule_viewport(
        &self,
        mut store: EditorStore,
        bounds: SheetRegionBoundsView,
        extent: SheetExtentView,
    ) {
        let Some(identity) = current_identity(store) else {
            return;
        };
        let tiles = tiles_for_region(bounds, extent.row_count, extent.column_count);
        let visible_keys = tiles
            .iter()
            .copied()
            .map(RegionTileKey::from_bounds)
            .collect::<Vec<_>>();
        store.region_cache.write().set_visible(visible_keys);

        let desired = tiles
            .into_iter()
            .filter(|tile| !store.region_cache.peek().contains(identity, *tile))
            .map(|bounds| RegionJobKey { identity, bounds })
            .collect::<HashSet<_>>();
        let should_start = {
            let mut state = self.0.state.borrow_mut();
            state.viewport = desired.clone();
            let required = state
                .required_waiters
                .keys()
                .copied()
                .collect::<HashSet<_>>();
            let mut removed = Vec::new();
            state.queue.retain(|job| {
                let keep = desired.contains(&job.key) || required.contains(&job.key);
                if !keep {
                    removed.push(job.key);
                }
                keep
            });
            for key in removed {
                state.scheduled.remove(&key);
            }
            for key in desired {
                if state.scheduled.insert(key) {
                    state.queue.push_back(RegionJob { key, store });
                }
            }
            trim_queue(&mut state);
            start_needed(&mut state)
        };
        if should_start {
            self.start();
        }
    }

    pub fn schedule_visible_rows(
        &self,
        mut store: EditorStore,
        sheet_index: usize,
        rows: &[usize],
        col_start: usize,
        col_end: usize,
        extent: SheetExtentView,
    ) {
        let Some(identity) = current_identity(store) else {
            return;
        };
        let tiles = sparse_row_regions(sheet_index, rows, col_start, col_end, extent);
        store.region_cache.write().set_visible(
            tiles
                .iter()
                .copied()
                .map(RegionTileKey::from_bounds)
                .collect::<Vec<_>>(),
        );
        let desired = tiles
            .into_iter()
            .filter(|tile| !store.region_cache.peek().contains(identity, *tile))
            .map(|bounds| RegionJobKey { identity, bounds })
            .collect::<HashSet<_>>();
        let should_start = {
            let mut state = self.0.state.borrow_mut();
            state.viewport = desired.clone();
            let required = state
                .required_waiters
                .keys()
                .copied()
                .collect::<HashSet<_>>();
            let mut removed = Vec::new();
            state.queue.retain(|job| {
                let keep = desired.contains(&job.key) || required.contains(&job.key);
                if !keep {
                    removed.push(job.key);
                }
                keep
            });
            for key in removed {
                state.scheduled.remove(&key);
            }
            for key in desired {
                if state.scheduled.insert(key) {
                    state.queue.push_back(RegionJob { key, store });
                }
            }
            trim_queue(&mut state);
            start_needed(&mut state)
        };
        if should_start {
            self.start();
        }
    }

    pub async fn ensure_region(
        &self,
        store: EditorStore,
        bounds: SheetRegionBoundsView,
        extent: SheetExtentView,
    ) -> Result<(), AppErrorDto> {
        let Some(identity) = current_identity(store) else {
            return Err(loader_error(
                "document_closed",
                "the workbook is no longer open",
            ));
        };
        let tiles = tiles_for_region(bounds, extent.row_count, extent.column_count);
        let mut receivers = Vec::new();
        let should_start = {
            let mut state = self.0.state.borrow_mut();
            for bounds in tiles {
                if store.region_cache.peek().contains(identity, bounds) {
                    continue;
                }
                let key = RegionJobKey { identity, bounds };
                let (sender, receiver) = oneshot::channel();
                state.required_waiters.entry(key).or_default().push(sender);
                receivers.push(receiver);
                if state.scheduled.insert(key) {
                    state.queue.push_front(RegionJob { key, store });
                } else if let Some(position) = state.queue.iter().position(|job| job.key == key) {
                    let job = state.queue.remove(position).expect("queued region exists");
                    state.queue.push_front(job);
                }
            }
            trim_queue(&mut state);
            start_needed(&mut state)
        };
        if should_start {
            self.start();
        }

        for receiver in receivers {
            receiver.await.map_err(|_| {
                loader_error(
                    "region_loader_stopped",
                    "the region loader stopped unexpectedly",
                )
            })??;
        }
        Ok(())
    }

    pub fn reset(&self) {
        let mut state = self.0.state.borrow_mut();
        state.queue.clear();
        state.viewport.clear();
        state.scheduled.clear();
        for (_, waiters) in state.required_waiters.drain() {
            resolve_waiters(
                waiters,
                Err(loader_error(
                    "region_request_superseded",
                    "the workbook changed while loading cells",
                )),
            );
        }
    }

    fn start(&self) {
        let loader = self.clone();
        spawn(async move {
            loader.run().await;
        });
    }

    async fn run(self) {
        loop {
            let Some(job) = self.0.state.borrow_mut().queue.pop_front() else {
                self.0.state.borrow_mut().running = false;
                return;
            };

            let jobs = self.take_rows_region_batch(job);
            let result = if jobs.len() == 1 && !is_single_row(jobs[0].key.bounds) {
                self.load(jobs[0]).await
            } else {
                self.load_rows(&jobs).await
            };
            let all_waiters = {
                let mut state = self.0.state.borrow_mut();
                jobs.iter()
                    .flat_map(|job| {
                        state.scheduled.remove(&job.key);
                        state.required_waiters.remove(&job.key).unwrap_or_default()
                    })
                    .collect::<Vec<_>>()
            };
            if all_waiters.is_empty()
                && let Err(error) = &result
                && error.code != "stale_region_response"
                && current_identity(job.store) == Some(job.key.identity)
            {
                job.store.set_error(error.clone());
            }
            resolve_waiters(all_waiters, result);
        }
    }

    fn take_rows_region_batch(&self, first: RegionJob) -> Vec<RegionJob> {
        if !is_single_row(first.key.bounds)
            || self
                .0
                .state
                .borrow()
                .required_waiters
                .contains_key(&first.key)
        {
            return vec![first];
        }
        let mut jobs = vec![first];
        let mut state = self.0.state.borrow_mut();
        let mut index = 0;
        while jobs.len() < MAX_ROWS_PER_REGION_REQUEST && index < state.queue.len() {
            let candidate = state.queue[index];
            let matches = is_single_row(candidate.key.bounds)
                && candidate.key.identity == first.key.identity
                && candidate.key.bounds.sheet_index == first.key.bounds.sheet_index
                && candidate.key.bounds.col_start == first.key.bounds.col_start
                && candidate.key.bounds.col_end == first.key.bounds.col_end
                && !state.required_waiters.contains_key(&candidate.key);
            if matches {
                jobs.push(
                    state
                        .queue
                        .remove(index)
                        .expect("matching sparse region job exists"),
                );
            } else {
                index += 1;
            }
        }
        jobs.sort_unstable_by_key(|job| job.key.bounds.row_start);
        jobs
    }

    async fn load(&self, mut job: RegionJob) -> Result<(), AppErrorDto> {
        let mut pending = vec![job.key.bounds];
        let mut fragments = Vec::new();
        let started = Instant::now();
        let mut requests = 0usize;

        while let Some(bounds) = pending.pop() {
            requests = requests.saturating_add(1);
            if requests > MAX_SPLIT_REQUESTS || started.elapsed() > SPLIT_DEADLINE {
                return Err(loader_error(
                    "region_split_limit",
                    "the requested cell region could not be loaded within its safety limit",
                ));
            }
            match self.request(job.key.identity, bounds).await {
                Ok(fragment) => fragments.push(fragment),
                Err(error) if error.code == "region_response_too_large" => {
                    let Some((first, second)) = split_bounds(bounds) else {
                        return Err(error);
                    };
                    pending.push(second);
                    pending.push(first);
                }
                Err(error) => return Err(error),
            }
        }

        if current_identity(job.store) != Some(job.key.identity) {
            return Err(loader_error(
                "stale_region_response",
                "the workbook changed before the cell region finished loading",
            ));
        }
        let inserted = job
            .store
            .region_cache
            .write()
            .insert_region(job.key.bounds, fragments);
        if inserted {
            Ok(())
        } else {
            Err(loader_error(
                "stale_region_response",
                "the cell region response did not match the current workbook",
            ))
        }
    }

    async fn load_rows(&self, jobs: &[RegionJob]) -> Result<(), AppErrorDto> {
        let mut first = jobs[0];
        let mut pending = VecDeque::from([jobs.to_vec()]);
        let mut loaded = Vec::new();
        let started = Instant::now();
        let mut requests = 0usize;
        while let Some(chunk) = pending.pop_front() {
            requests = requests.saturating_add(1);
            if requests > MAX_SPLIT_REQUESTS || started.elapsed() > SPLIT_DEADLINE {
                return Err(loader_error(
                    "region_split_limit",
                    "the requested physical rows could not be loaded within its safety limit",
                ));
            }
            match self.request_rows(&chunk).await {
                Ok(regions) => loaded.extend(
                    chunk
                        .into_iter()
                        .zip(regions)
                        .map(|(job, region)| (job.key.bounds, region)),
                ),
                Err(error) if error.code == "region_response_too_large" && chunk.len() > 1 => {
                    let second = chunk.split_at(chunk.len() / 2).1.to_vec();
                    let first = chunk[..chunk.len() / 2].to_vec();
                    pending.push_front(second);
                    pending.push_front(first);
                }
                Err(error) if error.code == "region_response_too_large" => {
                    self.load(chunk[0]).await?;
                }
                Err(error) => return Err(error),
            }
        }
        if current_identity(first.store) != Some(first.key.identity) {
            return Err(loader_error(
                "stale_region_response",
                "the workbook changed before the physical rows finished loading",
            ));
        }
        let mut cache = first.store.region_cache.write();
        if loaded
            .into_iter()
            .all(|(bounds, region)| cache.insert_region(bounds, vec![region]))
        {
            Ok(())
        } else {
            Err(loader_error(
                "stale_region_response",
                "the physical row response did not match the current workbook",
            ))
        }
    }

    async fn request_rows(&self, jobs: &[RegionJob]) -> Result<Vec<SheetRegionView>, AppErrorDto> {
        let first = jobs[0].key;
        let rows = jobs
            .iter()
            .map(|job| job.key.bounds.row_start)
            .collect::<Vec<_>>();
        let reply = self
            .0
            .editor
            .execute(EditorRequest::RowsRegion {
                document_id: first.identity.document_id,
                base_revision: first.identity.revision,
                sheet_index: first.bounds.sheet_index,
                rows,
                col_start: first.bounds.col_start,
                col_end: first.bounds.col_end,
            })
            .await?;
        let EditorReply::RowsRegion { value } = reply else {
            return Err(loader_error(
                "protocol_error",
                "the editor returned an unexpected physical row response",
            ));
        };
        let response = serde_json::from_value::<SheetRowsRegionView>(value).map_err(|error| {
            loader_error(
                "protocol_error",
                format!("invalid physical row response: {error}"),
            )
        })?;
        if response.regions.len() != jobs.len()
            || response.regions.iter().zip(jobs).any(|(region, job)| {
                region.document_id != job.key.identity.document_id
                    || region.revision != job.key.identity.revision
                    || region.region != job.key.bounds
                    || !valid_region_payload(region)
            })
        {
            return Err(loader_error(
                "stale_region_response",
                "the editor returned rows for a different workbook revision or region",
            ));
        }
        Ok(response.regions)
    }

    async fn request(
        &self,
        identity: DocumentRevision,
        bounds: SheetRegionBoundsView,
    ) -> Result<SheetRegionView, AppErrorDto> {
        let reply = self
            .0
            .editor
            .execute(EditorRequest::Region {
                document_id: identity.document_id,
                base_revision: identity.revision,
                sheet_index: bounds.sheet_index,
                row_start: bounds.row_start,
                row_end: bounds.row_end,
                col_start: bounds.col_start,
                col_end: bounds.col_end,
            })
            .await?;
        let EditorReply::Region { value } = reply else {
            return Err(loader_error(
                "protocol_error",
                "the editor returned an unexpected region response",
            ));
        };
        let region = serde_json::from_value::<SheetRegionView>(value).map_err(|error| {
            loader_error(
                "protocol_error",
                format!("invalid region response: {error}"),
            )
        })?;
        if region.document_id != identity.document_id
            || region.revision != identity.revision
            || region.region != bounds
            || !valid_region_payload(&region)
        {
            return Err(loader_error(
                "stale_region_response",
                "the editor returned cells for a different workbook revision or region",
            ));
        }
        Ok(region)
    }
}

fn sparse_row_regions(
    sheet_index: usize,
    rows: &[usize],
    col_start: usize,
    col_end: usize,
    extent: SheetExtentView,
) -> HashSet<SheetRegionBoundsView> {
    if col_start >= extent.column_count {
        return HashSet::new();
    }
    let first_col = col_start / SHEET_REGION_TILE_COLUMNS * SHEET_REGION_TILE_COLUMNS;
    rows.iter()
        .copied()
        .filter(|row| *row < extent.row_count)
        .flat_map(|row| {
            (first_col..col_end.min(extent.column_count))
                .step_by(SHEET_REGION_TILE_COLUMNS)
                .map(move |col| SheetRegionBoundsView {
                    sheet_index,
                    row_start: row,
                    row_end: row + 1,
                    col_start: col,
                    col_end: col
                        .saturating_add(SHEET_REGION_TILE_COLUMNS)
                        .min(extent.column_count),
                })
        })
        .collect()
}

fn is_single_row(bounds: SheetRegionBoundsView) -> bool {
    bounds.row_end == bounds.row_start.saturating_add(1)
}

fn valid_region_payload(region: &SheetRegionView) -> bool {
    region.region.row_start < region.region.row_end
        && region.region.col_start < region.region.col_end
        && region.cells.iter().all(|cell| {
            cell.sheet_index == region.region.sheet_index
                && cell.row >= region.region.row_start
                && cell.row < region.region.row_end
                && cell.col >= region.region.col_start
                && cell.col < region.region.col_end
        })
        && region
            .merge_anchor_cells
            .iter()
            .all(|cell| cell.sheet_index == region.region.sheet_index)
}

fn start_needed(state: &mut LoaderState) -> bool {
    if state.running || state.queue.is_empty() {
        false
    } else {
        state.running = true;
        true
    }
}

fn trim_queue(state: &mut LoaderState) {
    while state.queue.len() > MAX_QUEUED_REGIONS {
        let keys = state
            .queue
            .iter()
            .map(|job| job.key)
            .collect::<VecDeque<_>>();
        let required = state
            .required_waiters
            .keys()
            .copied()
            .collect::<HashSet<_>>();
        let Some(position) = optional_eviction_position(&keys, &required) else {
            break;
        };
        if let Some(job) = state.queue.remove(position) {
            state.scheduled.remove(&job.key);
        }
    }
}

fn optional_eviction_position(
    queue: &VecDeque<RegionJobKey>,
    required: &HashSet<RegionJobKey>,
) -> Option<usize> {
    queue.iter().rposition(|key| !required.contains(key))
}

fn split_bounds(
    bounds: SheetRegionBoundsView,
) -> Option<(SheetRegionBoundsView, SheetRegionBoundsView)> {
    let rows = bounds.row_end.saturating_sub(bounds.row_start);
    let columns = bounds.col_end.saturating_sub(bounds.col_start);
    if rows >= columns && rows > 1 {
        let middle = bounds.row_start + rows / 2;
        Some((
            SheetRegionBoundsView {
                row_end: middle,
                ..bounds
            },
            SheetRegionBoundsView {
                row_start: middle,
                ..bounds
            },
        ))
    } else if columns > 1 {
        let middle = bounds.col_start + columns / 2;
        Some((
            SheetRegionBoundsView {
                col_end: middle,
                ..bounds
            },
            SheetRegionBoundsView {
                col_start: middle,
                ..bounds
            },
        ))
    } else {
        None
    }
}

fn current_identity(store: EditorStore) -> Option<DocumentRevision> {
    store
        .document
        .peek()
        .as_ref()
        .map(|document| DocumentRevision {
            document_id: document.editor_session.document_id,
            revision: document.editor_session.revision,
        })
}

fn resolve_waiters(waiters: Vec<Waiter>, result: Result<(), AppErrorDto>) {
    for waiter in waiters {
        let _ = waiter.send(result.clone());
    }
}

fn loader_error(code: &str, message: impl Into<String>) -> AppErrorDto {
    AppErrorDto {
        code: code.to_string(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_regions_split_on_the_longest_axis() {
        let bounds = SheetRegionBoundsView {
            sheet_index: 2,
            row_start: 10,
            row_end: 110,
            col_start: 3,
            col_end: 23,
        };
        let (first, second) = split_bounds(bounds).expect("splittable");

        assert_eq!((first.row_start, first.row_end), (10, 60));
        assert_eq!((second.row_start, second.row_end), (60, 110));
        assert_eq!(first.col_start, 3);
        assert_eq!(second.col_end, 23);
    }

    #[test]
    fn a_single_cell_region_cannot_split() {
        assert!(
            split_bounds(SheetRegionBoundsView {
                sheet_index: 0,
                row_start: 4,
                row_end: 5,
                col_start: 7,
                col_end: 8,
            })
            .is_none()
        );
    }

    #[test]
    fn queue_eviction_keeps_required_regions() {
        let identity = DocumentRevision {
            document_id: 1,
            revision: 2,
        };
        let key = |row_start| RegionJobKey {
            identity,
            bounds: SheetRegionBoundsView {
                sheet_index: 0,
                row_start,
                row_end: row_start + 10,
                col_start: 0,
                col_end: 10,
            },
        };
        let required_key = key(10);
        let queue = VecDeque::from([key(0), required_key, key(20)]);
        let required = HashSet::from([required_key]);

        assert_eq!(optional_eviction_position(&queue, &required), Some(2));
        assert_eq!(
            optional_eviction_position(
                &VecDeque::from([required_key]),
                &HashSet::from([required_key]),
            ),
            None
        );
    }

    #[test]
    fn region_payload_rejects_cells_outside_the_response_bounds() {
        let value = serde_json::json!({
            "documentId": "1",
            "revision": "2",
            "region": {
                "sheetIndex": 0,
                "rowStart": 10,
                "rowEnd": 20,
                "colStart": 5,
                "colEnd": 10
            },
            "cells": [{
                "sheetIndex": 0,
                "row": 20,
                "col": 5,
                "value": { "raw": "outside" }
            }],
            "metadata": {},
            "wireBytes": 100
        });
        let region = serde_json::from_value(value).expect("valid response shape");

        assert!(!valid_region_payload(&region));
    }

    #[test]
    fn sparse_rows_keep_physical_coordinates_and_column_tiles() {
        let regions = sparse_row_regions(
            2,
            &[3, 700, 1_500],
            5,
            50,
            SheetExtentView {
                row_count: 2_000,
                column_count: 80,
            },
        );

        assert_eq!(regions.len(), 6);
        assert!(regions.contains(&SheetRegionBoundsView {
            sheet_index: 2,
            row_start: 700,
            row_end: 701,
            col_start: 0,
            col_end: 32,
        }));
        assert!(regions.contains(&SheetRegionBoundsView {
            sheet_index: 2,
            row_start: 1_500,
            row_end: 1_501,
            col_start: 32,
            col_end: 64,
        }));
    }
}
