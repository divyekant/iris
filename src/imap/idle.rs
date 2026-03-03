use std::time::Duration;
use tokio::task::JoinHandle;

use crate::ai::ollama::OllamaClient;
use crate::db::DbPool;
use crate::imap::connection::{connect, ImapCredentials};
use crate::imap::sync::SyncEngine;
use crate::ws::hub::WsHub;

/// Spawn a background task that keeps an IMAP IDLE connection open for
/// real-time push notifications.
///
/// The loop:
/// 1. Connect + authenticate + SELECT INBOX
/// 2. Enter IDLE (blocks until server notifies or 29-min timeout)
/// 3. On notification → break IDLE → re-sync → repeat
/// 4. On error → log, wait 30s, retry
pub fn spawn_idle_listener(
    account_id: String,
    creds: ImapCredentials,
    db: DbPool,
    ws_hub: WsHub,
    ollama: OllamaClient,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tracing::info!(account_id, "IDLE: connecting");

            let result = idle_loop(&account_id, &creds, &db, &ws_hub, &ollama).await;

            if let Err(e) = result {
                tracing::error!(account_id, error = %e, "IDLE loop error, retrying in 30s");
            }

            // Back off before reconnecting
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    })
}

/// Single iteration of the IDLE connection lifecycle.
async fn idle_loop(
    account_id: &str,
    creds: &ImapCredentials,
    db: &DbPool,
    ws_hub: &WsHub,
    ollama: &OllamaClient,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // 1. Connect
    let mut session = connect(creds).await?;

    // 2. SELECT INBOX
    session.select("INBOX").await?;

    tracing::info!(account_id, "IDLE: entering IDLE mode");

    // 3. Enter IDLE — consumes session, gives us a Handle
    let mut idle_handle = session.idle();
    idle_handle.init().await?;

    // 4. Wait for server notification (29-minute timeout per RFC 2177)
    let (idle_future, _stop_source) = idle_handle.wait_with_timeout(Duration::from_secs(29 * 60));
    let idle_result = idle_future.await?;

    tracing::info!(account_id, ?idle_result, "IDLE: got response");

    // 5. Exit IDLE — get session back
    let session = idle_handle.done().await?;

    // 6. Re-sync to pick up new messages
    tracing::info!(account_id, "IDLE: re-syncing after notification");

    let engine = SyncEngine {
        db: db.clone(),
        ws_hub: ws_hub.clone(),
        ollama: ollama.clone(),
    };

    // Re-run initial sync (INSERT OR IGNORE handles duplicates)
    if let Err(e) = engine.initial_sync(account_id, creds).await {
        tracing::error!(account_id, error = %e, "IDLE: re-sync failed");
    }

    // Logout the IDLE session (sync created its own connection)
    drop(session);

    Ok(())
}
