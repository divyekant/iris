# The Iris Mac app

`Iris.app` opens the web client of the local engine in a window of its own,
and shows the notices of Iris on the Mac. The design is in
`docs/specs/2026-11-17-mac-app.md` (card R6-06).

- The app does not hold an engine. It uses the engine that
  `iris service install` starts, in the default data folder
  `~/Library/Application Support/iris`.
- The app signs in with a new one-time code from the engine socket. You do
  not need the log of the service.
- Close the window: Iris stays in the menu bar. A dot on the icon shows
  notices that are not done. The web view ends with the window, so the app
  uses the least memory while you do not look at it.
- "Quit Iris" in the menu bar menu stops the app. The engine continues.

## Build the app

The app builds on macOS only. CI has no macOS job: CI tests
`src/engine.rs` on Linux.

1. Install the Tauri command one time:

   ```bash
   cargo install tauri-cli --version 2.12.4 --locked
   ```

2. Build the app:

   ```bash
   cd apps/mac && cargo tauri build
   ```

3. Copy `target/release/bundle/macos/Iris.app` to `/Applications`, then open
   it. Do not open the app from the build folder or from a download folder:
   in a temporary or translocated folder, macOS does not know the app by its
   ID, so the notices show with no click action.

The build makes an app with an ad-hoc signature. It is not notarized
(issue #542).

## Files

| Path | Content |
|---|---|
| `src/engine.rs` | The socket client, the paths and the state of the notices. Standard library and `serde_json` only. |
| `src/mac.rs` | The window, the menu bar icon and the notices. macOS only. |
| `ui/` | The local pages: "Iris is not running" and "Install Iris first". |
| `capabilities/engine.json` | The one Tauri permission of the engine page: a drag of the window. |
| `icons/` | The app icon (`icon@2x.png`, 1024 px) and the menu bar icons. |
