use std::env;
use std::fmt;

#[derive(Clone)]
pub struct Config {
    pub port: u16,
    pub database_url: String,
    pub ollama_url: String,
    pub memories_url: String,
    pub memories_api_key: Option<String>,
    pub anthropic_api_key: Option<String>,
    pub openai_api_key: Option<String>,
    pub gmail_client_id: Option<String>,
    pub gmail_client_secret: Option<String>,
    pub outlook_client_id: Option<String>,
    pub outlook_client_secret: Option<String>,
    pub app_password_hash: Option<String>,
    pub public_url: String,
    pub job_poll_interval_ms: u64,
    pub job_max_concurrency: usize,
    pub job_cleanup_days: i64,
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("port", &self.port)
            .field("database_url", &self.database_url)
            .field("ollama_url", &self.ollama_url)
            .field("memories_url", &self.memories_url)
            .field("memories_api_key", &"[REDACTED]")
            .field("anthropic_api_key", &"[REDACTED]")
            .field("openai_api_key", &"[REDACTED]")
            .field("gmail_client_id", &self.gmail_client_id)
            .field("gmail_client_secret", &"[REDACTED]")
            .field("outlook_client_id", &self.outlook_client_id)
            .field("outlook_client_secret", &"[REDACTED]")
            .field("app_password_hash", &"[REDACTED]")
            .field("public_url", &self.public_url)
            .field("job_poll_interval_ms", &self.job_poll_interval_ms)
            .field("job_max_concurrency", &self.job_max_concurrency)
            .field("job_cleanup_days", &self.job_cleanup_days)
            .finish()
    }
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            port: env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(3000),
            database_url: env::var("DATABASE_URL").unwrap_or_else(|_| "./data/iris.db".into()),
            ollama_url: env::var("OLLAMA_URL").unwrap_or_else(|_| "http://localhost:11434".into()),
            memories_url: env::var("MEMORIES_URL").unwrap_or_else(|_| "http://localhost:8900".into()),
            memories_api_key: env::var("MEMORIES_API_KEY").ok(),
            anthropic_api_key: env::var("ANTHROPIC_API_KEY").ok(),
            openai_api_key: env::var("OPENAI_API_KEY").ok(),
            gmail_client_id: env::var("GMAIL_CLIENT_ID").ok(),
            gmail_client_secret: env::var("GMAIL_CLIENT_SECRET").ok(),
            outlook_client_id: env::var("OUTLOOK_CLIENT_ID").ok(),
            outlook_client_secret: env::var("OUTLOOK_CLIENT_SECRET").ok(),
            app_password_hash: env::var("IRIS_AUTH_PASSWORD_HASH").ok(),
            public_url: env::var("PUBLIC_URL").unwrap_or_else(|_| "http://localhost:3000".into()),
            job_poll_interval_ms: env::var("JOB_POLL_INTERVAL_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(2000),
            job_max_concurrency: env::var("JOB_MAX_CONCURRENCY").ok().and_then(|v| v.parse().ok()).unwrap_or(4),
            job_cleanup_days: env::var("JOB_CLEANUP_DAYS").ok().and_then(|v| v.parse().ok()).unwrap_or(7),
        }
    }
}
