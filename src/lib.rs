#[cfg(feature = "app")]
mod actions;
#[cfg(feature = "app")]
mod components;
#[cfg(feature = "app")]
mod model;
#[cfg(feature = "app")]
mod ports;
#[cfg(feature = "app")]
mod ui;

#[cfg(feature = "app")]
pub use simple_table_engine::protocol;

#[cfg(any(
    all(feature = "desktop", feature = "mobile"),
    all(feature = "desktop", feature = "web"),
    all(feature = "desktop", feature = "server"),
    all(feature = "mobile", feature = "web"),
    all(feature = "mobile", feature = "server"),
    all(feature = "web", feature = "server"),
    all(feature = "worker", feature = "desktop"),
    all(feature = "worker", feature = "mobile"),
    all(feature = "worker", feature = "web"),
    all(feature = "worker", feature = "server"),
    all(feature = "tools", feature = "desktop"),
    all(feature = "tools", feature = "mobile"),
    all(feature = "tools", feature = "web"),
    all(feature = "tools", feature = "server"),
    all(feature = "tools", feature = "worker"),
))]
compile_error!("enable exactly one Simple Table target feature");

#[cfg(not(any(
    feature = "desktop",
    feature = "mobile",
    feature = "web",
    feature = "server",
    feature = "worker",
    feature = "tools"
)))]
compile_error!("enable one Simple Table target feature");

#[cfg(feature = "app")]
use std::rc::Rc;

#[cfg(feature = "app")]
use dioxus::prelude::*;

#[cfg(feature = "app")]
use model::{AppPorts, EditorStore};

#[cfg(feature = "app")]
const APP_CSS: Asset = asset!("/assets/main.css");
#[cfg(feature = "app")]
const FAVICON: Asset = asset!("/assets/favicon.png");
#[cfg(feature = "app")]
const LUCIDE_FONT: Asset = asset!("/assets/lucide.ttf");

#[cfg(feature = "app")]
#[derive(Clone, Debug, PartialEq, Routable)]
enum Route {
    #[route("/")]
    Home {},
    #[route("/table")]
    Table {},
}

#[cfg(feature = "app")]
pub fn app() -> Element {
    let store = use_hook(EditorStore::new);
    let ports = use_hook(|| {
        Rc::new(AppPorts {
            editor: ports::editor::platform_editor_port(),
            files: ports::file::platform_file_port(),
        })
    });
    use_context_provider(|| store);
    use_context_provider(|| Rc::clone(&ports));

    rsx! {
        dioxus::document::Stylesheet { href: APP_CSS }
        dioxus::document::Style {
            r#"
                @font-face {{
                    font-family: "Lucide Icons";
                    src: url("{LUCIDE_FONT}") format("truetype");
                    font-display: block;
                }}
            "#
        }
        dioxus::document::Link { rel: "icon", r#type: "image/png", href: FAVICON }
        dioxus::document::Title { "Simple Table" }
        Router::<Route> {}
        components::ErrorNotice {}
    }
}

#[cfg(feature = "app")]
#[component]
fn Home() -> Element {
    rsx! { components::HomeView {} }
}

#[cfg(feature = "app")]
#[component]
fn Table() -> Element {
    rsx! { components::EditorView {} }
}
