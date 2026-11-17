//! The Mac app of Iris (card R6-06, `docs/specs/2026-11-17-mac-app.md`).

mod engine;
#[cfg(target_os = "macos")]
mod mac;

fn main() {
    #[cfg(target_os = "macos")]
    mac::run();
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!("The Iris Mac app runs on macOS only.");
        std::process::exit(1);
    }
}
