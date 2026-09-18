//! Internal read-only snapshots assembled from engine state.
//!
//! These types sit between the live [`crate::state::EditorState`] and the wire
//! DTOs in [`crate::protocol`]. `protocol_projection` maps these snapshots to
//! protocol responses; they are never serialized directly.

mod document;
mod mutation;
mod mutation_retention;
mod status;

pub(crate) use document::*;
pub(crate) use mutation::*;
pub(crate) use mutation_retention::*;
pub(crate) use status::*;
