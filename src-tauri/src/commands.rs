pub mod common;
pub mod android;
pub mod ios;

pub use common::*;

#[cfg(target_os = "android")]
pub use android::{pick_file_android, read_file_android, save_file_android, pick_save_location_android};

#[cfg(target_os = "ios")]
pub use ios::{pick_file_ios, create_private_file_ios, save_file_ios, export_file_ios, silent_export_file_ios};
