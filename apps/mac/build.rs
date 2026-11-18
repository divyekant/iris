fn main() {
    // Only the macOS build is the app. On Linux, CI tests the logic of
    // `src/engine.rs` only.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        tauri_build::build();
    }
}
