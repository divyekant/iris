use iris_server::{ai, build_app, config::Config, db, jobs, ws, AppState};
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

    let memories = ai::memories::MemoriesClient::new(&config.memories_url, config.memories_api_key.clone());

    // Build LLM provider pool from configured keys
    let providers = {
        let mut providers = Vec::new();

        // Read Ollama model from DB config (if set)
        let ollama_model = pool
            .get()
            .ok()
            .and_then(|conn| {
                conn.query_row(
                    "SELECT value FROM config WHERE key = 'ai_model'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .ok()
            })
            .unwrap_or_default();

        // Ollama (always added — may or may not be running)
        let ollama = ai::ollama::OllamaClient::with_model(&config.ollama_url, &ollama_model);
        providers.push(ai::provider::LlmProvider::Ollama(ollama));

        // Anthropic (from env or DB config)
        let anthropic_key = config.anthropic_api_key.clone().or_else(|| {
            pool.get().ok().and_then(|conn| {
                conn.query_row("SELECT value FROM config WHERE key = 'anthropic_api_key'", [], |row| row.get::<_, String>(0)).ok()
            })
        });
        if let Some(ref key) = anthropic_key {
            if !key.is_empty() {
                let model = pool.get().ok().and_then(|conn| {
                    conn.query_row("SELECT value FROM config WHERE key = 'ai_model_anthropic'", [], |row| row.get::<_, String>(0)).ok()
                });
                let client = ai::anthropic::AnthropicClient::new(key, model.as_deref());
                providers.push(ai::provider::LlmProvider::Anthropic(client));
                tracing::info!("Anthropic provider configured");
            }
        }

        // OpenAI (from env or DB config)
        let openai_key = config.openai_api_key.clone().or_else(|| {
            pool.get().ok().and_then(|conn| {
                conn.query_row("SELECT value FROM config WHERE key = 'openai_api_key'", [], |row| row.get::<_, String>(0)).ok()
            })
        });
        if let Some(ref key) = openai_key {
            if !key.is_empty() {
                let model = pool.get().ok().and_then(|conn| {
                    conn.query_row("SELECT value FROM config WHERE key = 'ai_model_openai'", [], |row| row.get::<_, String>(0)).ok()
                });
                let client = ai::openai::OpenAIClient::new(key, model.as_deref());
                providers.push(ai::provider::LlmProvider::OpenAI(client));
                tracing::info!("OpenAI provider configured");
            }
        }

        tracing::info!("LLM provider pool: {} providers", providers.len());
        ai::provider::ProviderPool::new(providers)
    };

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
        providers,
        memories,
        session_token,
    });

    // Spawn the background job worker
    let worker = Arc::new(jobs::worker::JobWorker::new(
        state.db.clone(),
        state.ws_hub.clone(),
        state.providers.clone(),
        state.memories.clone(),
        config.job_poll_interval_ms,
        config.job_max_concurrency,
        config.job_cleanup_days,
    ));
    tokio::spawn(worker.run());

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
