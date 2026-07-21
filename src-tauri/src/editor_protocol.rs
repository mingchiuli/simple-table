/// Static editor protocol policy shared by Rust execution and generated
/// TypeScript consumers.
pub use crate::resource_limits::{MAX_CELL_TEXT_BYTES, MAX_MUTATION_TEXT_BYTES};

pub const EDITOR_MUTATION_PROTOCOL_VERSION: u16 = 4;
pub const MAX_MUTATION_RESPONSE_BYTES: usize = 3 * 1024 * 1024;
pub const MAX_SHEET_REGION_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_SEARCH_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_SEARCH_QUERY_BYTES: usize = 4 * 1024;
pub const MAX_SET_CELL_CHANGES: usize = 4_096;
