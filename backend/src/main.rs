mod api;
mod error;
mod fileio;
mod models;
mod paths;
mod patterns;
mod services;

use axum::{
    routing::get,
    Router,
    Json,
};
use sqlx::sqlite::SqlitePoolOptions;
use std::net::SocketAddr;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use tower_http::services::ServeDir;
use tower_http::cors::{CorsLayer, Any};
use std::path::PathBuf;
use serde::Serialize;

#[derive(Serialize)]
struct HealthResponse {
    name: String,
    version: String,
    status: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    dotenvy::dotenv().ok();

    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "sqlite:claude_registry.db".to_string());

    // Setup database pool
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await?;

    // Determine frontend path
    let mut frontend_dist = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    frontend_dist.pop(); // Up from backend
    frontend_dist.push("frontend/dist");

    let index_path = frontend_dist.join("index.html");

    // Configure CORS
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Build our application
    let app = Router::new()
        .route("/health", get(health))
        .nest("/api/v1", api::v1::router(pool))
        .fallback_service(
            ServeDir::new(frontend_dist)
                .append_index_html_on_directories(true)
                .not_found_service(tower_http::services::ServeFile::new(index_path))
        )
        .layer(cors);

    // Run it with hyper
    let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = std::env::var("PORT").unwrap_or_else(|_| "8000".to_string());
    let addr: SocketAddr = format!("{}:{}", host, port).parse()?;

    tracing::info!("listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        name: "Claude Deck".to_string(),
        version: "1.2.0".to_string(),
        status: "running".to_string(),
    })
}
