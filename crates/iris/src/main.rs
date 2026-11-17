use clap::Parser;

fn main() -> std::process::ExitCode {
    // The hold helper of a session (docs/specs/2026-11-17-session-launcher.md):
    // only the engine starts it, with the data folder, the session and the
    // agent command. The isolation of R4-05 reads the data folder (args[2]).
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    if args.get(1).is_some_and(|a| a == iris::SESSION_HOLD) {
        if args.len() < 3 {
            eprintln!("iris: session-hold needs the data folder");
            return std::process::ExitCode::from(iris_session::hold::BAD_ARGS as u8);
        }
        let code = iris_session::macos_job::helper_main(std::path::Path::new(&args[2]), &args[3..]);
        return std::process::ExitCode::from(code.clamp(0, 255) as u8);
    }
    // The job process of a macOS session, which launchd starts.
    if args
        .get(1)
        .is_some_and(|a| iris_session::macos_job::is_job(a))
    {
        if args.len() < 3 {
            eprintln!("iris: session-job needs the data folder");
            return std::process::ExitCode::from(iris_session::hold::BAD_ARGS as u8);
        }
        let code = iris_session::macos_job::job_main(std::path::Path::new(&args[2]), &args[3..]);
        return std::process::ExitCode::from(code.clamp(0, 255) as u8);
    }
    match iris::run(iris::Cli::parse()) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("iris: {e}");
            std::process::ExitCode::from(e.code)
        }
    }
}
