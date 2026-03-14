use iris_server::{ai, auth, build_app, config::Config, db, imap, jobs, models, secrets, ws, AppState};
use std::net::SocketAddr;
use std::sync::Arc;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter("iris_server=debug,info")
        .init();

    let config = Config::from_env();
    secrets::configure_from_env().expect("Failed to configure secrets encryption");
    let pool = db::create_pool(&config.database_url).expect("Failed to create database pool");

    // Run migrations
    {
        let conn = pool.get().expect("Failed to get DB connection");
        db::migrations::run(&conn).expect("Failed to run migrations");
        let report = secrets::migrate_persisted_secrets(&conn).expect("Failed to migrate persisted secrets");
        if report.migrated_values > 0 {
            tracing::info!(migrated = report.migrated_values, "Encrypted persisted secrets at rest");
        } else if report.plaintext_values_remaining > 0 {
            tracing::warn!(
                plaintext = report.plaintext_values_remaining,
                "Secrets remain plaintext at rest; set IRIS_SECRETS_KEY to encrypt persisted credentials"
            );
        }
    }

    let (memories_url, memories_api_key) = pool
        .get()
        .ok()
        .map(|conn| {
            let url = conn
                .query_row("SELECT value FROM config WHERE key = 'memories_url'", [], |row| row.get::<_, String>(0))
                .ok()
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| config.memories_url.clone());
            let key = secrets::get_secret_config_value(&conn, "memories_api_key")
                .ok()
                .flatten()
                .filter(|value| !value.is_empty())
                .or_else(|| config.memories_api_key.clone());
            (url, key)
        })
        .unwrap_or_else(|| (config.memories_url.clone(), config.memories_api_key.clone()));

    let memories = ai::memories::MemoriesClient::new(&memories_url, memories_api_key);

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
                secrets::get_secret_config_value(&conn, "anthropic_api_key")
                    .ok()
                    .flatten()
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
                secrets::get_secret_config_value(&conn, "openai_api_key")
                    .ok()
                    .flatten()
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

    // Generate a random session secret for this server instance.
    let session_token: String = (0..32)
        .map(|_| format!("{:02x}", rand::random::<u8>()))
        .collect();
    tracing::info!("Session cookie secret generated");

    // Optionally write the session secret to a file for trusted local scripts.
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
        state.config.clone(),
        state.ws_hub.clone(),
        state.providers.clone(),
        state.memories.clone(),
        config.job_poll_interval_ms,
        config.job_max_concurrency,
        config.job_cleanup_days,
    ));
    tokio::spawn(worker.run());

    // Spawn IMAP sync + IDLE for all existing accounts on startup
    {
        let conn = state.db.get().expect("DB connection for startup sync");
        let accounts = models::account::Account::list(&conn);
        drop(conn); // Release connection before async work
        for account in accounts {
            let (Some(imap_host), Some(imap_port)) = (account.imap_host.clone(), account.imap_port) else {
                continue;
            };
            let state_clone = state.clone();
            let acct_id = account.id.clone();
            let email = account.email.clone();
            tokio::spawn(async move {
                // Refresh OAuth token if needed
                let token = match auth::refresh::ensure_fresh_token(
                    &state_clone.db,
                    &account,
                    &state_clone.config,
                ).await {
                    Ok(Some(t)) => t,
                    Ok(None) => {
                        // Password-based account — use stored password
                        // (not yet implemented for startup sync)
                        tracing::warn!(account_id = %acct_id, "Skipping non-OAuth account for startup sync");
                        return;
                    }
                    Err(e) => {
                        tracing::error!(account_id = %acct_id, error = %e, "Failed to refresh token for startup sync");
                        return;
                    }
                };

                let creds = imap::connection::ImapCredentials {
                    host: imap_host,
                    port: imap_port as u16,
                    username: email,
                    auth: imap::connection::ImapAuth::OAuth2 { access_token: token },
                };

                tracing::info!(account_id = %acct_id, "Startup: syncing account");
                let engine = imap::sync::SyncEngine::new(
                    state_clone.db.clone(),
                    state_clone.ws_hub.clone(),
                );
                if let Err(e) = engine.initial_sync(&acct_id, &creds).await {
                    tracing::error!(account_id = %acct_id, error = %e, "Startup sync failed");
                }
                // Start IDLE listener for real-time push
                imap::idle::spawn_idle_listener(acct_id, creds, state_clone.db.clone(), state_clone.ws_hub.clone());
            });
        }
    }

    let app = build_app(state);

    let host: [u8; 4] = if std::env::var("BIND_ALL").is_ok() {
        [0, 0, 0, 0]
    } else {
        [127, 0, 0, 1]
    };
    let addr = SocketAddr::from((host, config.port));
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
