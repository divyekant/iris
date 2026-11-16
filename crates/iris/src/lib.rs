//! The Iris engine and the `iris` command.

use std::net::SocketAddr;

use clap::{Parser, Subcommand};

/// The default address of `iris serve`. Only this machine can connect to it.
pub const DEFAULT_ADDR: &str = "127.0.0.1:4800";

#[derive(Parser)]
#[command(name = "iris", version = VERSION, about = "Iris: local, agent-first email.")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Start the engine and serve the web client.
    Serve {
        /// The IP address and port to listen on, for example 127.0.0.1:4800.
        #[arg(long, default_value = DEFAULT_ADDR)]
        addr: SocketAddr,
    },
    /// Install or remove the background service that runs `iris serve`.
    Service {
        #[command(subcommand)]
        action: ServiceAction,
    },
}

#[derive(Subcommand)]
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

/// Return all the routes of the engine.
pub fn app() -> axum::Router {
    iris_web::router()
}

/// Serve the engine on `addr` until the process gets Ctrl-C.
pub fn serve(addr: SocketAddr) -> std::io::Result<()> {
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
            panic!("not serve");
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
}
