pub mod ops;
pub mod store;
pub mod thumbnail;
pub mod types;

pub use ops::{do_add_recent_file_with_thumbnail, do_get_recent_files, do_remove_recent_file};
pub use types::{AddRecentFileRequest, RecentFile};
