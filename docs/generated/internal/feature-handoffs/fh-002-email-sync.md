---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
feature: email-sync
slug: fh-002-email-sync
---

# Feature Handoff: Email Sync

## What It Does

Email sync pulls messages from a remote IMAP server into the local SQLite database and keeps the inbox up to date in real time using IMAP IDLE push notifications. It also triggers AI classification and Memories storage for each synced message.

## How It Works

### IMAP Connection (`src/imap/connection.rs`)

All IMAP connections use TLS (async-native-tls) over TCP with a 30-second connection timeout. Two authentication methods are supported:

- **XOAUTH2**: For Gmail and Outlook. Constructs a SASL XOAUTH2 authentication string (`user={email}\x01auth=Bearer {token}\x01\x01`), base64-encodes it, and sends it via the `AUTHENTICATE XOAUTH2` command.
- **Password**: Standard `LOGIN` command for manual IMAP accounts.

### Initial Sync (`SyncEngine::initial_sync` in `src/imap/sync.rs`)

1. Updates account sync_status to "syncing" and broadcasts progress via WebSocket.
2. Connects to IMAP, selects INBOX.
3. Fetches the newest 100 messages (sequence range `max(1, total-99):total`).
4. For each fetched message:
   - Parses the IMAP FETCH response (envelope, headers, body, flags).
   - Extracts MIME parts using `mailparse` (text/plain, text/html, attachments).
   - Extracts thread ID from headers (References first message-id, then In-Reply-To, then own Message-ID).
   - Inserts into the `messages` table (INSERT OR IGNORE handles duplicates via unique id).
   - Broadcasts `NewEmail` WebSocket event.
   - Spawns background AI classification (semaphore-limited to 4 concurrent tasks).
   - Spawns background Memories storage (fire-and-forget).
   - Broadcasts sync progress (fraction complete).
5. Updates sync_status to "idle" and broadcasts `SyncComplete`.

### IDLE Push (`src/imap/idle.rs`)

After initial sync completes, an IDLE listener runs in a perpetual loop:

1. Connects to IMAP, selects INBOX.
2. Enters IDLE mode with a 29-minute timeout (per RFC 2177).
3. Blocks until the server sends a notification (new message, flag change, etc.) or the timeout expires.
4. On notification, exits IDLE and re-runs initial sync to pick up new messages.
5. On error, logs and retries with exponential backoff (30s initial, capped at 15 minutes).

The backoff resets to 30 seconds after a successful IDLE cycle.

### WebSocket Notifications

The sync engine broadcasts events through `WsHub`:

- `SyncStatus { account_id, status, progress }` -- during sync
- `SyncComplete { account_id }` -- when sync finishes
- `NewEmail { account_id, message_id }` -- for each new message
- `AiProcessed { message_id }` -- when AI classification completes

### AI Processing Pipeline

Each synced message is passed to the AI pipeline in a background Tokio task, limited by a semaphore (`MAX_AI_CONCURRENCY = 4`). The pipeline:

1. Checks if AI is enabled and a model is configured (reads `ai_enabled` and `ai_model` from the config table).
2. Builds feedback context from user corrections (if any patterns with count >= 2 exist).
3. Calls `pipeline::process_email` which sends subject, from, and truncated body (2000 chars) to Ollama.
4. Parses the JSON response into `AiMetadata` (intent, priority, category, summary, entities, deadline).
5. Updates the message row with AI metadata.
6. Broadcasts `AiProcessed` event.

### Memories Storage

Each synced message is stored in the Memories vector store for semantic search:

- Text format: `From: {name} <{email}>\nSubject: {subject}\nDate: {date}\n\n{body (4000 char limit)}`
- Source: `iris/{account_id}/messages/{db_message_id}`
- Key: RFC Message-ID (for deduplication)

## User-Facing Behavior

- After connecting an account, the inbox progressively fills with messages. A progress indicator shows sync status.
- New messages arrive in real time via IDLE push -- the inbox updates without manual refresh.
- AI classification badges appear on messages shortly after sync (asynchronous).

## Configuration

| Variable | Default | Description |
|---|---|---|
| `OLLAMA_URL` | `http://localhost:11434` | Ollama API endpoint for AI classification |
| `MEMORIES_URL` | `http://localhost:8900` | Memories MCP server for semantic storage |

AI processing is controlled by the `ai_enabled` and `ai_model` keys in the `config` table, set via the Settings UI.

## Edge Cases and Limitations

- Initial sync fetches only the newest 100 messages. Older messages are not synced.
- IDLE only monitors the INBOX folder. Other folders (Sent, Drafts, etc.) are not monitored.
- If the IMAP connection drops during sync, the account status is set to "error" with the error message stored in `sync_error`.
- INSERT OR IGNORE on the messages table means duplicate syncs are safe but do not update existing messages (flag changes from the server are not reflected).
- The AI semaphore limits concurrent classification to 4 tasks to avoid overwhelming the Ollama instance.
- If Ollama is unreachable or AI is disabled, classification is silently skipped.
- If Memories is unreachable, storage is silently skipped (fire-and-forget).
- OAuth tokens are refreshed before IMAP connection but not during long-running IDLE sessions. If a token expires during the 29-minute IDLE window, the re-sync will use a fresh connection with a refreshed token.

## Common Questions

**Q: How often does IDLE reconnect?**
A: The IDLE timeout is 29 minutes (per RFC 2177 recommendation). After each timeout or server notification, the IDLE connection is torn down and a fresh one is established after re-sync.

**Q: What happens if Ollama is slow and backs up?**
A: The semaphore limits concurrent AI tasks to 4. Additional tasks queue up waiting for a semaphore permit. If the Ollama request itself times out (controlled by the HTTP client timeout), the task completes silently without updating AI metadata.

**Q: Can I trigger a manual re-sync?**
A: There is no explicit re-sync API endpoint. The IDLE loop continuously monitors for changes. A server restart will trigger initial sync for all accounts.

## Troubleshooting

| Symptom | Likely Cause | Resolution |
|---|---|---|
| Account stuck in "syncing" status | IMAP connection hung or crashed | Restart the server; check IMAP host/port accessibility |
| No new emails appearing | IDLE loop crashed or provider blocked IDLE | Check server logs for IDLE errors; verify IMAP IDLE support |
| Sync completes but AI badges missing | Ollama unreachable or AI disabled | Check `ai_enabled` config; verify Ollama health at `/api/health` |
| "IMAP connection timeout after 30s" | Network issue or wrong IMAP host/port | Verify IMAP host and port; check firewall/proxy settings |
| Exponential backoff messages in logs | Repeated IMAP connection failures | Check credentials, token expiration, network connectivity |

## Related Links

- Source: `src/imap/sync.rs`, `src/imap/idle.rs`, `src/imap/connection.rs`
- AI pipeline: `src/ai/pipeline.rs`
- Memories: `src/ai/memories.rs`
- WebSocket: `src/ws/`
- Database: `migrations/001_initial.sql` (messages table)
