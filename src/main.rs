#[cfg(not(feature = "server"))]
fn main() {
    dioxus::launch(simple_table::app);
}

#[cfg(feature = "server")]
#[tokio::main]
async fn main() {
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use axum::{Router, routing::get};
    use dioxus::server::{DioxusRouterExt, ServeConfig};

    let ip = std::env::var("IP")
        .ok()
        .and_then(|value| value.parse::<IpAddr>().ok())
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
    let port = std::env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(8080);
    let address = SocketAddr::new(ip, port);
    let router = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .serve_dioxus_application(ServeConfig::new(), simple_table::app);
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .expect("failed to bind SSR listener");
    axum::serve(listener, router)
        .await
        .expect("SSR server failed");
}
