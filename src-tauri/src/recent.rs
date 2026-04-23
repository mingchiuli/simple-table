pub mod types;
pub mod store;
pub mod thumbnail;
pub mod ops;

pub use types::{StorageType, RecentFile};
pub use ops::{
    do_get_recent_files,
    do_add_recent_file_with_thumbnail,
    do_remove_recent_file,
    do_check_file_exists,
    do_update_recent_file_path,
};