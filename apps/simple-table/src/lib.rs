mod actions;
mod components;
mod model;
mod ports;
mod ui;

pub use simple_table_protocol as protocol;

#[cfg(any(
    all(feature = "desktop", feature = "mobile"),
    all(feature = "desktop", feature = "web"),
    all(feature = "desktop", feature = "server"),
    all(feature = "mobile", feature = "web"),
    all(feature = "mobile", feature = "server"),
    all(feature = "web", feature = "server"),
))]
compile_error!("enable exactly one Simple Table target feature");

#[cfg(not(any(
    feature = "desktop",
    feature = "mobile",
    feature = "web",
    feature = "server",
)))]
compile_error!("enable one Simple Table target feature");

use std::rc::Rc;

use dioxus::prelude::*;

use model::{AppPorts, EditorStore};

const APP_CSS: Asset = asset!("/assets/main.css");
const FAVICON: Asset = asset!("/assets/favicon.png");

#[derive(Clone, Debug, PartialEq, Routable)]
enum Route {
    #[route("/")]
    Home {},
    #[route("/table")]
    Table {},
}

pub fn app() -> Element {
    let store = use_hook(EditorStore::new);
    let ports = use_hook(|| {
        let editor = ports::editor::platform_editor_port();
        Rc::new(AppPorts {
            regions: actions::RegionLoader::new(Rc::clone(&editor)),
            editor,
            files: ports::file::platform_file_port(),
            #[cfg(feature = "mobile")]
            recovery: ports::recovery::platform_recovery_port(),
            operations: Rc::new(futures::lock::Mutex::new(())),
        })
    });
    use_context_provider(|| store);
    use_context_provider(|| Rc::clone(&ports));
    #[cfg(all(feature = "mobile", target_os = "android"))]
    use_effect(|| {
        let _ = ports::android::configure_system_bars();
    });
    let platform_class = if cfg!(target_os = "android") {
        "app-root platform-android"
    } else {
        "app-root"
    };

    rsx! {
        dioxus::document::Stylesheet { href: simple_table_components::DX_COMPONENTS_THEME }
        dioxus::document::Stylesheet { href: APP_CSS }
        dioxus::document::Link { rel: "icon", r#type: "image/png", href: FAVICON }
        dioxus::document::Title { "Simple Table" }
        simple_table_components::ToastProvider {
            div { class: platform_class,
                Router::<Route> {}
                components::ErrorToastBridge {}
            }
        }
    }
}

#[component]
fn Home() -> Element {
    rsx! { components::HomeView {} }
}

#[component]
fn Table() -> Element {
    rsx! { components::EditorView {} }
}
