use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use axum::Router;
#[cfg(feature = "embedded")]
use axum::extract::OriginalUri;
#[cfg(feature = "embedded")]
use axum::http::header::{CACHE_CONTROL, CONTENT_TYPE};
#[cfg(feature = "embedded")]
use axum::http::{HeaderValue, StatusCode};
#[cfg(feature = "embedded")]
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use dioxus::server::{DioxusRouterExt, ServeConfig};
#[cfg(feature = "embedded")]
use rust_embed::RustEmbed;

#[cfg(feature = "embedded")]
#[derive(RustEmbed)]
#[folder = "../../target/embedded-web-public/"]
#[cfg_attr(debug_assertions, allow_missing = true)]
struct WebAssets;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    #[cfg(feature = "embedded")]
    let public_dir = materialize_index()?;

    #[cfg(feature = "embedded")]
    // SAFETY: this runs before the application starts any worker threads.
    unsafe {
        std::env::set_var("DIOXUS_PUBLIC_PATH", public_dir.path())
    };

    let router = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .serve_dioxus_application(ServeConfig::new(), simple_table::app);
    #[cfg(feature = "embedded")]
    let router = router
        .route("/assets/{*path}", get(embedded_asset))
        .route("/workers/{*path}", get(embedded_asset));

    serve(router).await
}

#[cfg(feature = "embedded")]
fn materialize_index() -> Result<tempfile::TempDir, Box<dyn std::error::Error>> {
    let index = WebAssets::get("index.html")
        .ok_or("embedded index.html is missing; build with `cargo xtask bundle`")?;
    let directory = tempfile::Builder::new()
        .prefix("simple-table-web-")
        .tempdir()?;
    std::fs::write(directory.path().join("index.html"), index.data.as_ref())?;
    Ok(directory)
}

async fn serve(router: Router) -> Result<(), Box<dyn std::error::Error>> {
    let ip = std::env::var("IP")
        .unwrap_or_else(|_| IpAddr::V4(Ipv4Addr::LOCALHOST).to_string())
        .parse::<IpAddr>()?;
    let port = std::env::var("PORT")
        .unwrap_or_else(|_| "8080".to_owned())
        .parse::<u16>()?;
    let address = SocketAddr::new(ip, port);
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!("Simple Table listening on http://{address}");
    axum::serve(listener, router).await?;
    Ok(())
}

#[cfg(feature = "embedded")]
async fn embedded_asset(OriginalUri(uri): OriginalUri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let Some(asset) = WebAssets::get(path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let cache = if path.contains("-dxh") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };

    let mut response = asset.data.into_owned().into_response();
    let content_type = HeaderValue::from_str(mime.as_ref())
        .unwrap_or_else(|_| HeaderValue::from_static("application/octet-stream"));
    response.headers_mut().insert(CONTENT_TYPE, content_type);
    response
        .headers_mut()
        .insert(CACHE_CONTROL, HeaderValue::from_static(cache));
    response
}
