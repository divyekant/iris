//! The Iris engine and the `iris` command.

/// Write one line to stderr. A failed write (for example a closed pipe under
/// a service manager) is ignored: `eprintln!` panics, and that stops the engine.
macro_rules! say {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stderr(), $($arg)*);
    }};
}

pub mod engine;
pub mod verbs;

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
#[command(name = "iris", version = VERSION, about = "Iris: local, agent-first email.")]
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
    /// Sync or verify the mirror now, outside the schedule of `iris serve`.
    Mirror {
        #[command(subcommand)]
        command: MirrorCommand,
    },
    /// Show the accounts, the sync and the message counts.
    Status {
        /// Print the JSON of the local API (`/api/status`).
        #[arg(long)]
        json: bool,
    },
    /// Search the mail (specification 1, section 5.6), newest first.
    Search {
        /// The search text, for example "contract from:dana after:2026-10-01".
        text: String,
        /// The most messages to show, 1 to 200.
        #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u32).range(1..=200))]
        limit: u32,
        /// Print the JSON of the local API (`/api/search`).
        #[arg(long)]
        json: bool,
    },
    /// Show one message (`m-42`) or one item (`c-7`).
    Show {
        id: String,
        /// Print the JSON of the local API (`/api/messages/{id}`). An item is always JSON.
        #[arg(long)]
        json: bool,
    },
    /// Correct one field of an item by hand (`c-7 due.date 2026-01-09`). The
    /// correction stays through each later sync and understanding run.
    Correct {
        /// The item, for example `c-7`.
        id: String,
        /// The field, as a dotted path, for example `due.date` or
        /// `fields.amount.value`. Not `status`, `title` or another common field.
        field: String,
        /// The new value: JSON (`{"value": "12.50", "currency": "EUR"}`), or
        /// else a string.
        value: String,
    },
    /// Copy the data folder to a new folder while the engine runs.
    Backup {
        /// The backup folder. It must not exist, or must be empty.
        path: PathBuf,
    },
    /// Install or remove the background service that runs `iris serve`.
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
}

#[derive(Subcommand)]
pub enum MirrorCommand {
    /// Sync each account one time (specification 1, section 4.3).
    Sync,
    /// Compare the mirror of each account with Gmail, and check the local
    /// content (specification 1, section 4.8). A difference makes the next
    /// sync run a full resync.
    Verify,
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

#[derive(Clone, Copy, Subcommand)]
pub enum ServiceAction {
    /// Install the service for this program and start it now.
    Install,
    /// Stop the service and remove its file.
    Remove,
}

/// Run `iris service install` or `iris service remove`. Return the message
/// for the user.
pub fn service(action: ServiceAction) -> std::io::Result<String> {
    match action {
        ServiceAction::Install => {
            // The service file holds this path, so give the real file, not a link.
            let program = std::env::current_exe()?.canonicalize()?;
            let file = iris_service::install(&program)?;
            Ok(format!(
                "iris: installed and started the service ({})",
                file.display()
            ))
        }
        ServiceAction::Remove => Ok(match iris_service::remove()? {
            Some(file) => format!("iris: stopped the service and removed {}", file.display()),
            None => "iris: no service is installed".to_string(),
        }),
    }
}

/// The version of the program: the release tag that `deploy.yml` sets in
/// `IRIS_RELEASE` at build time, or the crate version for other builds.
pub const VERSION: &str = match option_env!("IRIS_RELEASE") {
    Some(tag) => tag,
    None => env!("CARGO_PKG_VERSION"),
};

/// Return the version line that `iris --version` prints.
pub fn version_line() -> String {
    format!("iris {VERSION}")
}

/// Run one command. Gives the error text for standard error.
pub fn run(cli: Cli) -> Result<(), String> {
    let data = || engine::data_dir(cli.data.clone()).map_err(|e| e.to_string());
    let open =
        || -> Result<Store, String> { Store::open(&data()?).map_err(|e| format!("store: {e}")) };
    match &cli.command {
        Command::Serve { addr } => {
            // Check before the store opens, so a refused address makes no data folder.
            check_loopback(*addr).map_err(|e| e.to_string())?;
            serve(Arc::new(open()?), *addr).map_err(|e| e.to_string())
        }
        Command::Account {
            command: AccountCommand::Add { client, paste },
        } => account_add(&open()?, client, *paste),
        Command::Mirror { command } => mirror(Arc::new(open()?), command),
        Command::Status { json } => print(verbs::status(&open()?, *json)),
        Command::Search { text, limit, json } => {
            print(verbs::search(&open()?, text, *limit, *json))
        }
        Command::Show { id, json } => print(verbs::show(&open()?, id, *json)),
        Command::Correct { id, field, value } => {
            print(verbs::correct(&open()?, id, field, value, engine::now_ms()))
        }
        Command::Backup { path } => {
            let files = open()?.backup(path).map_err(|e| format!("backup: {e}"))?;
            println!("iris: backup in {} ({files} content files)", path.display());
            Ok(())
        }
        Command::Service { action } => {
            println!("{}", service(*action).map_err(|e| e.to_string())?);
            Ok(())
        }
    }
}

/// `iris mirror sync|verify`: each account that has a sign-in, one at a time.
/// If another process syncs an account, that account gives an error.
fn mirror(store: Arc<Store>, command: &MirrorCommand) -> Result<(), String> {
    let config = gmail_config()?;
    let accounts = engine::signed_in_accounts(&store).map_err(|e| e.to_string())?;
    if accounts.is_empty() {
        return Err("no account to sync: run `iris account add <client.json>`".into());
    }
    let mut failed = false;
    for (account, state) in accounts {
        // Report each account, so a success means that each mailbox ran.
        if state.as_deref() == Some(engine::STATE_NEEDS_SIGN_IN) {
            failed = true;
            say!("iris: account {account}: {}", engine::Error::NeedsSignIn);
            continue;
        }
        let mut worker = engine::Worker::new(
            store.clone(),
            iris_gmail::Gmail::new(config.clone()),
            account,
            std::env::var("IRIS_TEST_TOKEN_URI").ok(),
        );
        let line = match command {
            MirrorCommand::Sync => worker.sync().map(|s| {
                format!(
                    "synced to history {}: fetched {}, gone {}, failed {}{}",
                    s.history_id,
                    s.fetched,
                    s.gone,
                    s.failed,
                    if s.resync { ", full resync" } else { "" }
                )
            }),
            MirrorCommand::Verify => worker.verify().map(|r| {
                let mut text = format!(
                    "verified at history {}: {} differences, {} skipped{}",
                    r.history_id,
                    r.total,
                    r.skipped,
                    if r.resync {
                        ", the next sync runs a full resync"
                    } else {
                        ""
                    }
                );
                for d in &r.differences {
                    text.push_str(&format!("\n  {}", verbs::field(&d.to_string())));
                }
                text
            }),
        };
        match line {
            Ok(text) => println!("account {account}: {text}"),
            Err(e) => {
                failed = true;
                say!("iris: account {account}: {e}");
            }
        }
    }
    if failed {
        Err("one or more accounts failed".into())
    } else {
        Ok(())
    }
}

fn print(out: verbs::Output) -> Result<(), String> {
    print!("{}", out?);
    Ok(())
}

/// `iris account add`: sign in, then save the client and the sealed refresh token.
fn account_add(store: &Store, client_path: &std::path::Path, paste: bool) -> Result<(), String> {
    let json = std::fs::read(client_path).map_err(|e| format!("{}: {e}", client_path.display()))?;
    let client = test_token_uri(iris_auth::Client::from_json(&json).map_err(|e| e.to_string())?)?;
    // Check the Gmail settings before the sign-in, so a bad setting fails
    // before a token exists.
    let gmail = gmail()?;
    let sign_in = client.sign_in().map_err(|e| e.to_string())?;
    say!(
        "iris: open this address in a browser and sign in:\n{}",
        sign_in.url()
    );
    let tokens = if paste {
        say!("iris: then paste the address of the browser here:");
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
        engine::save_sign_in(store, &gmail, &json, &tokens).map_err(|e| e.to_string())?;
    println!("iris: added {address}");
    if tokens.refresh_expires_in.is_some() {
        say!(
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
    gmail_config().map(iris_gmail::Gmail::new)
}

fn gmail_config() -> Result<iris_gmail::Config, String> {
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
        if !is_local_url(&url) {
            return Err("IRIS_TEST_GMAIL_URL must be http://127.0.0.1:<port>".into());
        }
        config.base_url = url;
    }
    Ok(config)
}

/// True for `http://127.0.0.1:<port>` with an optional path. The host part
/// ends at the first `/`, so `http://127.0.0.1:80@other.host/` is refused.
pub fn is_local_url(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("http://127.0.0.1:") else {
        return false;
    };
    let port = rest.split('/').next().unwrap_or_default();
    !port.is_empty() && port.len() <= 5 && port.bytes().all(|b| b.is_ascii_digit())
}

/// Tests set `IRIS_TEST_TOKEN_URI` to a fake token endpoint on 127.0.0.1.
fn test_token_uri(client: iris_auth::Client) -> Result<iris_auth::Client, String> {
    match std::env::var("IRIS_TEST_TOKEN_URI") {
        Ok(uri) => client.with_test_token_uri(&uri).map_err(|e| e.to_string()),
        Err(_) => Ok(client),
    }
}

/// Return all the routes of the engine.
pub fn app(store: Arc<Store>, auth: Arc<iris_api::Auth>) -> axum::Router {
    // The API routes come before the fallback of the web client.
    iris_api::router(store, auth).merge(iris_web::router())
}

/// The API has no TLS, so only this machine may connect (specification 2,
/// section 8.2).
fn check_loopback(addr: SocketAddr) -> std::io::Result<()> {
    if addr.ip().is_loopback() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "{addr} is not a loopback address: use 127.0.0.1 or [::1]"
        )))
    }
}

/// Serve the engine on `addr` until the process gets Ctrl-C. A thread runs
/// the sync schedule.
pub fn serve(store: Arc<Store>, addr: SocketAddr) -> std::io::Result<()> {
    check_loopback(addr)?;
    let gmail = gmail_config().map_err(std::io::Error::other)?;
    let api_store = store.clone();
    let sync = engine::Engine::new(store, gmail, std::env::var("IRIS_TEST_TOKEN_URI").ok());
    std::thread::Builder::new()
        .name("sync".into())
        .spawn(move || sync.run())?;

    // One thread: the lightest memory footprint has priority.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .build()?;
    rt.block_on(async move {
        let listener = tokio::net::TcpListener::bind(addr).await?;
        let local = listener.local_addr()?;
        // The API checks the Host against the bound address.
        let auth = Arc::new(iris_api::Auth::new(local)?);
        say!("iris: serving on http://{local}");
        // The one-time code of the API sign-in (specification 2, section 8.6).
        if let Some(url) = auth.sign_in_url() {
            say!("iris: sign-in address of the local API: {url}");
        }
        axum::serve(listener, app(api_store, auth))
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
    fn cli_parses_service_install_and_remove() {
        for (arg, install) in [("install", true), ("remove", false)] {
            let cli = Cli::try_parse_from(["iris", "service", arg]).unwrap();
            let Command::Service { action } = cli.command else {
                panic!("not service");
            };
            assert_eq!(matches!(action, ServiceAction::Install), install);
        }
    }

    #[test]
    fn serve_refuses_an_address_that_is_not_loopback() {
        let dir = std::env::temp_dir().join(format!("iris-serve-open-{}", std::process::id()));
        let store = Arc::new(Store::open(&dir).unwrap());
        for addr in ["0.0.0.0:0", "192.0.2.1:4800", "[::]:0"] {
            let err = serve(store.clone(), addr.parse().unwrap()).unwrap_err();
            assert!(
                err.to_string().contains("not a loopback address"),
                "{addr}: {err}"
            );
        }
    }

    #[test]
    fn only_a_loopback_url_is_local() {
        assert!(is_local_url("http://127.0.0.1:4800"));
        assert!(is_local_url("http://127.0.0.1:4800/gmail/v1"));
        for bad in [
            "http://127.0.0.1:80@evil.example",
            "http://127.0.0.1:80@evil.example/gmail/v1",
            "http://127.0.0.1:/x",
            "http://127.0.0.1:80x/",
            "http://127.0.0.1.evil.example:80/",
            "https://evil.example/",
        ] {
            assert!(!is_local_url(bad), "{bad}");
        }
    }

    #[test]
    fn cli_parses_the_data_folder_after_the_command() {
        let cli = Cli::try_parse_from(["iris", "backup", "/tmp/b", "--data", "/tmp/d"]).unwrap();
        assert_eq!(cli.data, Some(PathBuf::from("/tmp/d")));
    }
}
