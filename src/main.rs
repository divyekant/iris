mod config;

use config::Config;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt().with_env_filter("iris_server=debug,info").init();

    let config = Config::from_env();
    tracing::info!("Iris starting on port {}", config.port);
}
