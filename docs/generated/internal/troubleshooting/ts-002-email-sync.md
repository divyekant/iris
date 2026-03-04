---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
troubleshooting: email-sync
slug: ts-002-email-sync
---

# Troubleshooting: Email Sync

## Overview

This guide covers IMAP connection failures, IDLE push notification issues, sync errors, and WebSocket notification problems.

## Diagnostic Checklist

1. **Account sync status**: Check `GET /api/accounts` and look at the `sync_status` field. Values: `pending`, `syncing`, `idle`, `error`.
2. **Health endpoint**: `GET /api/health` shows overall system status including database availability.
3. **Server logs**: Look for `[IDLE]`, `[sync]`, or `account_id` in log output. Set `RUST_LOG=iris_server=debug` for verbose output.
4. **WebSocket connection**: Open browser DevTools > Network > WS to verify the WebSocket connection is established and receiving events.

## Issue: Account stuck in "syncing" status

**Symptoms**: The account shows "syncing" status indefinitely. No new messages appear.

**Causes and resolutions**:

| Cause | Resolution |
|---|---|
| IMAP connection hung | Restart the Iris server. The 30-second TCP connection timeout should prevent indefinite hangs, but IMAP protocol-level hangs are possible. |
| IMAP server rate limiting | Some providers throttle IMAP connections. Wait a few minutes and restart. |
| Database lock contention | With WAL mode and busy_timeout=5000ms, this should be rare. Check for other processes accessing the SQLite file. |
| Large mailbox causing slow FETCH | The initial sync fetches up to 100 messages. If messages are very large, FETCH may take a long time. Check server logs for progress. |

## Issue: No new emails appearing (IDLE not working)

**Symptoms**: Initial sync completed, but new emails are not arriving in real time.

**Causes and resolutions**:

| Cause | Resolution |
|---|---|
| IDLE loop crashed | Check logs for "IDLE loop error" messages. The loop retries with exponential backoff. |
| Provider does not support IDLE | Most modern providers support IDLE, but some may not. Check provider documentation. |
| OAuth token expired during IDLE | The IDLE session uses the token from connection time. If the 29-minute window exceeds the token lifetime, the next reconnection will refresh the token. |
| Firewall dropping idle connections | Some firewalls terminate idle TCP connections after a timeout (often less than 29 minutes). The IDLE timeout of 29 minutes should handle this, but aggressive firewalls may still cause issues. |
| IDLE listener not spawned | Check logs for "IDLE: connecting" messages. If absent, the listener was not started (possibly due to missing IMAP host/port). |

## Issue: "IMAP connection timeout after 30s"

**Symptoms**: Server logs show the timeout error. Account is in "error" status.

**Causes and resolutions**:

| Cause | Resolution |
|---|---|
| Wrong IMAP host or port | Verify the account's imap_host and imap_port values. |
| DNS resolution failure | Ensure the IMAP hostname resolves correctly. |
| Network/firewall blocking | Port 993 (IMAP TLS) must be accessible from the server. Check firewall rules. |
| Provider outage | Temporary issue. The IDLE loop will retry with backoff. |
| Corporate proxy/VPN | If behind Cloudflare WARP or a corporate proxy, IMAP connections may be blocked. Check proxy configuration. |

## Issue: XOAUTH2 authentication failure

**Symptoms**: Logs show authentication errors during IMAP connect. Error messages reference SASL or AUTHENTICATE.

**Causes and resolutions**:

| Cause | Resolution |
|---|---|
| Expired access token | Token should be refreshed before connecting. Check that `ensure_fresh_token` succeeds before sync. |
| Wrong email as username | XOAUTH2 uses the email address as the username. Verify the account's email field matches the OAuth-authenticated email. |
| IMAP scope not granted | Gmail requires the `https://mail.google.com/` scope. Outlook requires `IMAP.AccessAsUser.All`. Check that these scopes were requested during OAuth. |
| Account locked by provider | The provider may lock the account due to suspicious activity. Check the provider's security alerts. |

## Issue: Messages synced but missing content

**Symptoms**: Messages appear in the inbox but show empty body or missing subject.

**Causes and resolutions**:

| Cause | Resolution |
|---|---|
| MIME parsing failure | Check logs for mailparse errors. The system falls back to raw body text if MIME parsing fails. |
| Charset encoding issue | The `mailparse` crate handles common encodings (UTF-8, ISO-8859-1, etc.) but may fail on rare charsets. |
| Empty FETCH response | The IMAP server may return empty body for some messages. This is a server-side issue. |
| HTML-only email with DOMPurify stripping | If the email is HTML-only with no text part, and DOMPurify aggressively strips content, the visible content may appear minimal. |

## Issue: Exponential backoff messages flooding logs

**Symptoms**: Repeated "IDLE loop error, retrying" messages with increasing backoff times.

**Resolution**: This indicates persistent connection failures. Common causes:

1. **Credential issue**: Token expired and cannot be refreshed. Re-authenticate the account.
2. **Network issue**: IMAP server unreachable. Check connectivity.
3. **Provider blocking**: Too many rapid connection attempts triggered rate limiting. Wait for the backoff to reach its maximum (15 minutes) and the issue often resolves.

The backoff resets to 30 seconds after any successful IDLE cycle.

## Issue: WebSocket not receiving events

**Symptoms**: Sync completes in logs but the frontend does not update.

**Causes and resolutions**:

| Cause | Resolution |
|---|---|
| WebSocket not connected | Check DevTools > Network > WS. The connection should be to `/ws?token={session_token}`. |
| Session token mismatch | If the server restarted, the WebSocket token may be stale. Refresh the page. |
| CORS/proxy issue | If accessing via a reverse proxy, ensure WebSocket upgrades are forwarded. |

## Server Log Patterns

| Log Message | Meaning |
|---|---|
| `INBOX has N messages` (INFO) | Successfully selected INBOX, found N messages |
| `Fetching message range M:N` (INFO) | Fetching messages in the specified sequence range |
| `Fetched N messages` (INFO) | Successfully fetched N messages from IMAP |
| `Initial sync complete` (INFO) | Sync finished successfully |
| `IDLE: connecting` (INFO) | IDLE listener is (re)connecting |
| `IDLE: entering IDLE mode` (INFO) | IDLE command sent, waiting for server notification |
| `IDLE: got response` (INFO) | Server sent a notification or timeout occurred |
| `IDLE: re-syncing after notification` (INFO) | Re-sync triggered after IDLE notification |
| `IDLE loop error, retrying` (ERROR) | IDLE connection failed, will retry with backoff |
| `AI processing complete` (DEBUG) | Background AI classification finished for a message |
