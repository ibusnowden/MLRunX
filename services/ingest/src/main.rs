use axum::{Router, routing::get};
use std::net::SocketAddr;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

async fn health() -> &'static str {
    "ok"
}

async fn root() -> &'static str {
    "MLRunX Ingest Service v0.1.0"
}

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    // Build HTTP router (gRPC will be added later)
    let app = Router::new()
        .route("/", get(root))
        .route("/health", get(health));

    // Start HTTP server
    let http_addr = SocketAddr::from(([0, 0, 0, 0], 3002));
    info!("Starting MLRunX Ingest HTTP on {}", http_addr);
    info!("gRPC endpoint will be available on port 50051");

    let listener = tokio::net::TcpListener::bind(http_addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
