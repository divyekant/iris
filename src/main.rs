mod api;
mod auth;
mod config;
mod db;
mod models;
mod ws;

use axum::{Router, routing::{get, put}};
use config::Config;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};

pub struct AppState {
    pub db: db::DbPool,
    pub config: Config,
    pub ws_hub: ws::hub::WsHub,
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter("iris_server=debug,info")
        .init();

    let config = Config::from_env();
    let pool = db::create_pool(&config.database_url).expect("Failed to create database pool");

    // Run migrations
    {
        let conn = pool.get().expect("Failed to get DB connection");
        db::migrations::run(&conn).expect("Failed to run migrations");
    }

    let state = Arc::new(AppState {
        db: pool,
        config: config.clone(),
        ws_hub: ws::hub::WsHub::new(),
    });

    let api_routes = Router::new()
        .route("/health", get(api::health::health))
        .route("/accounts", get(api::accounts::list_accounts).post(api::accounts::create_account))
        .route("/accounts/{id}", get(api::accounts::get_account).delete(api::accounts::delete_account))
        .route("/messages", get(api::messages::list_messages))
        .route("/config", get(api::config::get_config))
        .route("/config/theme", put(api::config::set_theme))
        .route("/auth/oauth/{provider}", get(auth::oauth::start_oauth));

    let spa = ServeDir::new("web/dist").fallback(ServeFile::new("web/dist/index.html"));

    let app = Router::new()
        .route("/ws", get(ws::ws_handler))
        .route("/auth/callback", get(auth::oauth::oauth_callback))
        .nest("/api", api_routes)
        .fallback_service(spa)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    tracing::info!("Iris listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
