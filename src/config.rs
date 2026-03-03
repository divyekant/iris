use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub database_url: String,
    pub ollama_url: String,
    pub gmail_client_id: Option<String>,
    pub gmail_client_secret: Option<String>,
    pub outlook_client_id: Option<String>,
    pub outlook_client_secret: Option<String>,
    pub public_url: String,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            port: env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(3000),
            database_url: env::var("DATABASE_URL").unwrap_or_else(|_| "./data/iris.db".into()),
            ollama_url: env::var("OLLAMA_URL").unwrap_or_else(|_| "http://localhost:11434".into()),
            gmail_client_id: env::var("GMAIL_CLIENT_ID").ok(),
            gmail_client_secret: env::var("GMAIL_CLIENT_SECRET").ok(),
            outlook_client_id: env::var("OUTLOOK_CLIENT_ID").ok(),
            outlook_client_secret: env::var("OUTLOOK_CLIENT_SECRET").ok(),
            public_url: env::var("PUBLIC_URL").unwrap_or_else(|_| "http://localhost:3000".into()),
        }
    }
}
