use dioxus::prelude::*;

#[rustfmt::skip]
pub mod components;

pub use components::alert_dialog::*;
pub use components::badge::*;
pub use components::button::*;
pub use components::dialog::*;
pub use components::input::*;
pub use components::item::*;
pub use components::label::*;
pub use components::popover::*;
pub use components::scroll_area::*;
pub use components::separator::*;
pub use components::switch::*;
pub use components::tabs::*;
pub use components::toast::*;
pub use components::toolbar::*;
pub use components::tooltip::*;
pub use dioxus_icons::lucide as icons;
pub use dioxus_primitives::scroll_area::{ScrollDirection, ScrollType};
pub use dioxus_primitives::toast::{ToastOptions, use_toast};
pub use dioxus_primitives::{ContentAlign, ContentSide};

pub static DX_COMPONENTS_THEME: Asset = asset!("/assets/dx-components-theme.css");
