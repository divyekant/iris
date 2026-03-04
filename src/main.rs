use iris_server::{ai, build_app, config::Config, db, ws, AppState};
use std::net::SocketAddr;
use std::sync::Arc;

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
    let memories = ai::memories::MemoriesClient::new(&config.memories_url, config.memories_api_key.clone());

    // Generate a random session token for this server instance
    let session_token: String = (0..32)
        .map(|_| format!("{:02x}", rand::random::<u8>()))
        .collect();
    tracing::info!("Session token generated (use /api/auth/bootstrap to retrieve)");

    // Optionally write token to file for Docker/script integration
    if let Ok(path) = std::env::var("SESSION_TOKEN_FILE") {
        if let Err(e) = std::fs::write(&path, &session_token) {
            tracing::warn!("Failed to write session token to {}: {}", path, e);
        }
    }

    let state = Arc::new(AppState {
        db: pool,
        config: config.clone(),
        ws_hub: ws::hub::WsHub::new(),
        ollama,
        memories,
        session_token,
    });

    let app = build_app(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], config.port));
    tracing::info!("Iris listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .unwrap();

    tracing::info!("Iris shut down gracefully");
}

async fn shutdown_signal() {
    let ctrl_c = tokio::signal::ctrl_c();
    #[cfg(unix)]
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .expect("failed to install SIGTERM handler");
    #[cfg(unix)]
    let sigterm_recv = sigterm.recv();
    #[cfg(not(unix))]
    let sigterm_recv = std::future::pending::<Option<()>>();

    tokio::select! {
        _ = ctrl_c => tracing::info!("Received SIGINT, shutting down..."),
        _ = sigterm_recv => tracing::info!("Received SIGTERM, shutting down..."),
    }
}
