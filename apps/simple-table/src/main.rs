#[cfg(any(feature = "desktop", feature = "mobile"))]
fn main() {
    let index = include_str!("../index.html").replace("{app_title}", "Simple Table");
    dioxus::LaunchBuilder::new()
        .with_cfg(NativeConfig::new().with_custom_index(index))
        .launch(simple_table::app);
}

#[cfg(feature = "desktop")]
type NativeConfig = dioxus::desktop::Config;

#[cfg(feature = "mobile")]
type NativeConfig = dioxus::mobile::Config;

#[cfg(any(feature = "web", feature = "server"))]
fn main() {
    dioxus::launch(simple_table::app);
}
