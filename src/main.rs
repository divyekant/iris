mod ai;
mod api;
mod auth;
mod config;
mod db;
mod imap;
mod models;
mod smtp;
mod ws;

use axum::{Router, routing::{get, put, post, delete, patch}};
use config::Config;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};

pub struct AppState {
    pub db: db::DbPool,
    pub config: Config,
    pub ws_hub: ws::hub::WsHub,
    pub ollama: ai::ollama::OllamaClient,
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

    let ollama = ai::ollama::OllamaClient::new(&config.ollama_url);

    let state = Arc::new(AppState {
        db: pool,
        config: config.clone(),
        ws_hub: ws::hub::WsHub::new(),
        ollama,
    });

    let api_routes = Router::new()
        .route("/health", get(api::health::health))
        .route("/accounts", get(api::accounts::list_accounts).post(api::accounts::create_account))
        .route("/accounts/{id}", get(api::accounts::get_account).delete(api::accounts::delete_account))
        .route("/messages", get(api::messages::list_messages))
        .route("/messages/{id}", get(api::messages::get_message))
        .route("/messages/{id}/read", put(api::messages::mark_message_read))
        .route("/messages/batch", patch(api::messages::batch_update_messages))
        .route("/threads/{id}", get(api::threads::get_thread))
        .route("/threads/{id}/summarize", post(api::ai_actions::summarize_thread))
        .route("/search", get(api::search::search))
        .route("/ai/assist", post(api::ai_actions::ai_assist))
        .route("/ai/chat", post(api::chat::chat))
        .route("/ai/chat/{session_id}", get(api::chat::get_history))
        .route("/ai/chat/confirm", post(api::chat::confirm_action))
        .route("/config", get(api::config::get_config))
        .route("/config/theme", put(api::config::set_theme))
        .route("/config/view-mode", put(api::config::set_view_mode))
        .route("/config/ai", get(api::ai_config::get_ai_config).put(api::ai_config::set_ai_config))
        .route("/config/ai/test", post(api::ai_config::test_ai_connection))
        .route("/auth/oauth/{provider}", get(auth::oauth::start_oauth))
        .route("/send", post(api::compose::send_message))
        .route("/drafts", get(api::compose::list_drafts).post(api::compose::save_draft))
        .route("/drafts/{id}", delete(api::compose::delete_draft));

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
