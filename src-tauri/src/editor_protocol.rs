/// Static editor protocol policy shared by Rust execution and generated
/// TypeScript consumers.
pub const EDITOR_MUTATION_PROTOCOL_VERSION: u16 = 4;
pub const MAX_MUTATION_RESPONSE_BYTES: usize = 3 * 1024 * 1024;
pub const MAX_SHEET_REGION_RESPONSE_BYTES: usize = 16 * 1024 * 1024;
