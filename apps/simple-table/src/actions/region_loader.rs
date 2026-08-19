use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::time::{Duration, Instant};

use dioxus::prelude::{ReadableExt, WritableExt, spawn};
use futures::channel::oneshot;

use crate::model::region_cache::tiles_for_region;
use crate::model::{
    DocumentRevision, EditorStore, RegionTileKey, SheetExtentView, SheetRegionBoundsView,
    SheetRegionView,
};
use crate::ports::editor::EditorPort;
use crate::protocol::{AppErrorDto, EditorReply, EditorRequest};

const MAX_QUEUED_REGIONS: usize = 16;
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

            let result = self.load(job).await;
            let waiters = {
                let mut state = self.0.state.borrow_mut();
                state.scheduled.remove(&job.key);
                state.required_waiters.remove(&job.key).unwrap_or_default()
            };
            if waiters.is_empty()
                && let Err(error) = &result
                && error.code != "stale_region_response"
                && current_identity(job.store) == Some(job.key.identity)
            {
                job.store.set_error(error.clone());
            }
            resolve_waiters(waiters, result);
        }
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
}
