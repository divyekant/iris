use clap::Parser;

fn main() -> std::process::ExitCode {
    match iris::run(iris::Cli::parse()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("iris: {e}");
            std::process::ExitCode::from(e.code)
        }
    }
}
