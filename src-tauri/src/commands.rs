pub mod android;
pub mod common;
pub mod ios;
#[cfg(any(target_os = "android", target_os = "ios"))]
pub mod mobile;

pub use common::*;

#[cfg(target_os = "android")]
pub use android::{
    export_file_android, pick_file_android, pick_save_location_android, read_file_android,
    save_file_android,
};

#[cfg(target_os = "ios")]
pub use ios::{create_private_file_ios, export_file_ios, pick_file_ios, save_file_ios};

#[cfg(any(target_os = "android", target_os = "ios"))]
pub use mobile::check_update_mobile;
