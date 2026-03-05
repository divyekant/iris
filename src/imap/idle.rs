use std::time::Duration;
use tokio::task::JoinHandle;

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
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut backoff_secs = 30u64;
        loop {
            tracing::info!(account_id, "IDLE: connecting");

            let result = idle_loop(&account_id, &creds, &db, &ws_hub).await;

            match result {
                Ok(_) => {
                    // Successful IDLE cycle — reset backoff
                    backoff_secs = 30;
                }
                Err(e) => {
                    tracing::error!(account_id, error = %e, backoff_secs, "IDLE loop error, retrying");
                }
            }

            tokio::time::sleep(Duration::from_secs(backoff_secs)).await;
            // Exponential backoff capped at 15 minutes
            backoff_secs = (backoff_secs * 2).min(900);
        }
    })
}

/// Single iteration of the IDLE connection lifecycle.
async fn idle_loop(
    account_id: &str,
    creds: &ImapCredentials,
    db: &DbPool,
    ws_hub: &WsHub,
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
    let mut session = idle_handle.done().await?;

    // 6. Re-sync to pick up new messages
    tracing::info!(account_id, "IDLE: re-syncing after notification");

    let engine = SyncEngine::new(db.clone(), ws_hub.clone());

    // Re-run initial sync (INSERT OR IGNORE handles duplicates)
    if let Err(e) = engine.initial_sync(account_id, creds).await {
        tracing::error!(account_id, error = %e, "IDLE: re-sync failed");
    }

    // Logout the IDLE session (sync created its own connection)
    let _ = session.logout().await;

    Ok(())
}
