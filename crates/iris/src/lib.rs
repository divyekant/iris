//! The Iris engine and the `iris` command.

use std::net::SocketAddr;

use clap::{Parser, Subcommand};

/// The default address of `iris serve`. Only this machine can connect to it.
pub const DEFAULT_ADDR: &str = "127.0.0.1:4800";

#[derive(Parser)]
#[command(name = "iris", version, about = "Iris: local, agent-first email.")]
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
}

/// Return the version line that `iris --version` prints.
pub fn version_line() -> String {
    format!("iris {}", env!("CARGO_PKG_VERSION"))
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
        let Command::Serve { addr } = cli.command;
        assert_eq!(addr.to_string(), DEFAULT_ADDR);
    }
}
