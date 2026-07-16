pub mod check;

#[cfg(any(target_os = "android", target_os = "ios"))]
pub use check::check_update_mobile_impl;
