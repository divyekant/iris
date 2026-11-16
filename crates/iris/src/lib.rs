//! The Iris engine and the `iris` command.

pub mod engine;

use std::io::BufRead;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand};
use iris_store::Store;

/// The default address of `iris serve`. Only this machine can connect to it.
pub const DEFAULT_ADDR: &str = "127.0.0.1:4800";

/// The longest wait for the browser to come back after a sign-in.
pub const SIGN_IN_WAIT: Duration = Duration::from_secs(10 * 60);

#[derive(Parser)]
#[command(name = "iris", version, about = "Iris: local, agent-first email.")]
pub struct Cli {
    /// The data folder. The default is the variable IRIS_DATA, else the
    /// folder of the system (specification 1, section 5.1).
    #[arg(long, global = true)]
    pub data: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Start the engine: sync each account and serve the web client.
    Serve {
        /// The IP address and port to listen on, for example 127.0.0.1:4800.
        #[arg(long, default_value = DEFAULT_ADDR)]
        addr: SocketAddr,
    },
    /// Add or sign in again to a Gmail account.
    Account {
        #[command(subcommand)]
        command: AccountCommand,
    },
    /// Copy the data folder to a new folder while the engine runs.
    Backup {
        /// The backup folder. It must not exist, or must be empty.
        path: PathBuf,
    },
}

#[derive(Subcommand)]
pub enum AccountCommand {
    /// Sign in with the JSON file of your own OAuth client ("Desktop app").
    Add {
        /// The client file from the Google Cloud console.
        client: PathBuf,
        /// Paste the address of the browser after the sign-in, in place of
        /// the wait for the redirect. Use it when the browser is on another
        /// machine than the engine.
        #[arg(long)]
        paste: bool,
    },
}

/// Return the version line that `iris --version` prints.
pub fn version_line() -> String {
    format!("iris {}", env!("CARGO_PKG_VERSION"))
}

/// Run one command. Gives the error text for standard error.
pub fn run(cli: Cli) -> Result<(), String> {
    let data = || engine::data_dir(cli.data.clone()).map_err(|e| e.to_string());
    let open =
        || -> Result<Store, String> { Store::open(&data()?).map_err(|e| format!("store: {e}")) };
    match &cli.command {
        Command::Serve { addr } => serve(Arc::new(open()?), *addr).map_err(|e| e.to_string()),
        Command::Account {
            command: AccountCommand::Add { client, paste },
        } => account_add(&open()?, client, *paste),
        Command::Backup { path } => {
            let store = open()?;
            let files = store.backup(path).map_err(|e| format!("backup: {e}"))?;
            engine::backup_client(&store, path).map_err(|e| format!("backup: {e}"))?;
            println!("iris: backup in {} ({files} content files)", path.display());
            Ok(())
        }
    }
}

/// `iris account add`: sign in, then save the client and the sealed refresh token.
fn account_add(store: &Store, client_path: &std::path::Path, paste: bool) -> Result<(), String> {
    let json = std::fs::read(client_path).map_err(|e| format!("{}: {e}", client_path.display()))?;
    let client = test_token_uri(iris_auth::Client::from_json(&json).map_err(|e| e.to_string())?)?;
    let sign_in = client.sign_in().map_err(|e| e.to_string())?;
    eprintln!(
        "iris: open this address in a browser and sign in:\n{}",
        sign_in.url()
    );
    let tokens = if paste {
        eprintln!("iris: then paste the address of the browser here:");
        let mut line = String::new();
        std::io::stdin()
            .lock()
            .read_line(&mut line)
            .map_err(|e| e.to_string())?;
        sign_in.finish(line.trim())
    } else {
        sign_in.wait(SIGN_IN_WAIT)
    }
    .map_err(|e| e.to_string())?;
    let (_, address) =
        engine::save_sign_in(store, &gmail()?, &tokens).map_err(|e| e.to_string())?;
    engine::save_client(store, &json).map_err(|e| e.to_string())?;
    println!("iris: added {address}");
    if tokens.refresh_expires_in.is_some() {
        eprintln!(
            "iris: caution: the OAuth client is in the \"Testing\" status, so the sign-in \
             ends after 7 days. Publish the app to \"In production\"."
        );
    }
    Ok(())
}

/// The Gmail client. `IRIS_GMAIL_QUOTA` changes the quota units for each
/// minute, so a change at Google needs no new release. Tests set
/// `IRIS_TEST_GMAIL_URL` to a fake server on 127.0.0.1. Any other address is
/// refused, so the token cannot go elsewhere.
pub fn gmail() -> Result<iris_gmail::Gmail, String> {
    let mut config = iris_gmail::Config::default();
    if let Ok(quota) = std::env::var("IRIS_GMAIL_QUOTA") {
        config.quota_per_minute = quota
            .parse()
            .ok()
            .filter(|q| *q > 0)
            .ok_or("IRIS_GMAIL_QUOTA must be a number above 0")?;
    }
    if let Some(url) = std::env::var_os("IRIS_TEST_GMAIL_URL") {
        let url = url.into_string().unwrap_or_default();
        if !url.starts_with("http://127.0.0.1:") {
            return Err("IRIS_TEST_GMAIL_URL must be http://127.0.0.1:<port>".into());
        }
        config.base_url = url;
    }
    Ok(iris_gmail::Gmail::new(config))
}

/// Tests set `IRIS_TEST_TOKEN_URI` to a fake token endpoint on 127.0.0.1.
fn test_token_uri(client: iris_auth::Client) -> Result<iris_auth::Client, String> {
    match std::env::var("IRIS_TEST_TOKEN_URI") {
        Ok(uri) => client.with_test_token_uri(&uri).map_err(|e| e.to_string()),
        Err(_) => Ok(client),
    }
}

/// Return all the routes of the engine.
pub fn app() -> axum::Router {
    iris_web::router()
}

/// Serve the engine on `addr` until the process gets Ctrl-C. A thread runs
/// the sync schedule.
pub fn serve(store: Arc<Store>, addr: SocketAddr) -> std::io::Result<()> {
    let client = match engine::load_client(&store) {
        Ok(c) => Some(test_token_uri(c).map_err(std::io::Error::other)?),
        Err(engine::Error::NoClient) => None,
        Err(e) => return Err(std::io::Error::other(e.to_string())),
    };
    let gmail = gmail().map_err(std::io::Error::other)?;
    let sync = engine::Engine::new(store, gmail, client);
    std::thread::Builder::new()
        .name("sync".into())
        .spawn(move || engine::run(sync))?;

    // One thread: the lightest memory footprint has priority.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .build()?;
    rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        eprintln!("iris: serving on http://{}", listener.local_addr()?);
        axum::serve(listener, app())
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_line_names_the_program() {
        assert!(version_line().starts_with("iris "));
    }

    #[test]
    fn cli_parses_serve_with_default_addr() {
        let cli = Cli::try_parse_from(["iris", "serve"]).unwrap();
        let Command::Serve { addr } = cli.command else {
            panic!("not serve")
        };
        assert_eq!(addr.to_string(), DEFAULT_ADDR);
    }

    #[test]
    fn cli_parses_the_data_folder_after_the_command() {
        let cli = Cli::try_parse_from(["iris", "backup", "/tmp/b", "--data", "/tmp/d"]).unwrap();
        assert_eq!(cli.data, Some(PathBuf::from("/tmp/d")));
    }
}
