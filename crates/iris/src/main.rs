use clap::Parser;
use iris::{Cli, Command};

fn main() -> std::process::ExitCode {
    match Cli::parse().command {
        Command::Serve { addr } => match iris::serve(addr) {
            Ok(()) => std::process::ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("iris: {e}");
                std::process::ExitCode::FAILURE
            }
        },
        Command::Service { action } => match iris::service(action) {
            Ok(message) => {
                println!("{message}");
                std::process::ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("iris: {e}");
                std::process::ExitCode::FAILURE
            }
        },
    }
}
