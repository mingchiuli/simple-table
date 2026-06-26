#![allow(clippy::module_inception)]

pub mod content_hash;
pub mod editor_state;
pub mod search_index;
pub mod state;

pub use state::get_state;
