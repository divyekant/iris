# V2: Email Reader Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add thread view with full message rendering so users can read email conversations.

**Architecture:** Extend V1's IMAP sync with MIME parsing (`mailparse` crate) for proper body/attachment extraction and thread ID computation from References/In-Reply-To headers. Add thread and message detail API endpoints. New Svelte ThreadView page renders email bodies in sandboxed iframes with DOMPurify sanitization. Reply/Forward buttons are visible but disabled (V3 stubs).

**Tech Stack:** mailparse 0.15 (Rust MIME parsing), DOMPurify 3.x (HTML sanitization), sandboxed iframes

**V2 Affordances:** U13 (thread header), U15 (message list in thread), U16 (message body - sanitized HTML), U19 (attachment list), U22 (reply/forward stubs), N46 (WebSocket thread updates)

---

## Context for All Tasks

**Existing V1 state (do not modify these patterns, extend them):**
- Messages are synced via `src/imap/sync.rs:parse_fetch()` which currently sets `body_html: None`, `has_attachments: false`, `thread_id: None`
- `MessageSummary` (in `src/models/message.rs`) is the lightweight inbox struct (no body fields)
- `InsertMessage` struct has fields for `body_text`, `body_html`, `attachment_names`, `thread_id` — all currently unused or None
- API routes are defined in `src/main.rs:44-51`, handlers in `src/api/`
- Frontend uses `svelte-spa-router` hash routing in `web/src/App.svelte`
- `web/src/lib/api.ts` has a `request<T>()` helper for typed fetches
- All tests use `crate::db::create_test_pool()` for in-memory SQLite

---

### Task 1: MIME Parsing + Thread ID in Sync Engine

Add `mailparse` crate for MIME parsing. Parse email body into text/plain, text/html, and attachment metadata. Extract thread_id from References/In-Reply-To headers.

**Files:**
- Modify: `Cargo.toml` (add mailparse dependency)
- Modify: `src/models/message.rs` (add AttachmentMeta struct)
- Modify: `src/imap/sync.rs` (add MIME parsing, thread ID extraction, update parse_fetch)

**Step 1: Add mailparse dependency**

In `Cargo.toml`, add under `[dependencies]`:

```toml
mailparse = "0.15"
```

**Step 2: Add AttachmentMeta struct to models/message.rs**

Add this struct above `MessageSummary` (after the existing imports):

```rust
/// Attachment metadata stored as JSON in the attachment_names column.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttachmentMeta {
    pub filename: String,
    pub mime_type: String,
    pub size: usize,
}
```

**Step 3: Write failing tests in sync.rs**

Add a `#[cfg(test)]` module at the bottom of `src/imap/sync.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mime_plain_text() {
        let headers = "Content-Type: text/plain; charset=\"UTF-8\"\r\n";
        let body = "Hello, this is a plain text email.";
        let result = parse_mime_body(headers, body);
        assert!(result.text.is_some());
        assert!(result.text.unwrap().contains("Hello"));
        assert!(result.html.is_none());
        assert!(result.attachments.is_empty());
    }

    #[test]
    fn test_parse_mime_multipart() {
        let headers = "Content-Type: multipart/alternative; boundary=\"b123\"\r\n";
        let body = "--b123\r\nContent-Type: text/plain\r\n\r\nPlain version\r\n--b123\r\nContent-Type: text/html\r\n\r\n<p>HTML version</p>\r\n--b123--";
        let result = parse_mime_body(headers, body);
        assert!(result.text.is_some());
        assert!(result.text.unwrap().contains("Plain version"));
        assert!(result.html.is_some());
        assert!(result.html.unwrap().contains("<p>HTML version</p>"));
    }

    #[test]
    fn test_parse_mime_with_attachment() {
        let headers = "Content-Type: multipart/mixed; boundary=\"mix\"\r\n";
        let body = "--mix\r\nContent-Type: text/plain\r\n\r\nSee attached.\r\n--mix\r\nContent-Type: application/pdf; name=\"report.pdf\"\r\nContent-Disposition: attachment; filename=\"report.pdf\"\r\n\r\nfakepdfdata\r\n--mix--";
        let result = parse_mime_body(headers, body);
        assert!(result.text.is_some());
        assert_eq!(result.attachments.len(), 1);
        assert_eq!(result.attachments[0].filename, "report.pdf");
        assert_eq!(result.attachments[0].mime_type, "application/pdf");
    }

    #[test]
    fn test_extract_thread_id_with_references() {
        let headers = "Message-ID: <msg3@ex.com>\r\nReferences: <msg1@ex.com> <msg2@ex.com>\r\nIn-Reply-To: <msg2@ex.com>\r\n";
        let msg_id = Some("<msg3@ex.com>".to_string());
        let tid = extract_thread_id(headers, &msg_id);
        assert_eq!(tid, Some("msg1@ex.com".to_string()));
    }

    #[test]
    fn test_extract_thread_id_with_in_reply_to_only() {
        let headers = "Message-ID: <reply@ex.com>\r\nIn-Reply-To: <original@ex.com>\r\n";
        let msg_id = Some("<reply@ex.com>".to_string());
        let tid = extract_thread_id(headers, &msg_id);
        assert_eq!(tid, Some("original@ex.com".to_string()));
    }

    #[test]
    fn test_extract_thread_id_standalone() {
        let headers = "Message-ID: <solo@ex.com>\r\n";
        let msg_id = Some("<solo@ex.com>".to_string());
        let tid = extract_thread_id(headers, &msg_id);
        assert_eq!(tid, Some("solo@ex.com".to_string()));
    }
}
```

**Step 4: Run tests to verify they fail**

Run: `cargo test imap::sync::tests -- --nocapture 2>&1`
Expected: FAIL — functions `parse_mime_body` and `extract_thread_id` don't exist yet.

**Step 5: Implement MIME parsing and thread ID extraction**

Add these functions to `src/imap/sync.rs` (above `parse_fetch`), and add the necessary import at the top:

```rust
use mailparse::MailHeaderMap;
use crate::models::message::AttachmentMeta;
```

Add a local struct for parsed results:

```rust
struct ParsedBody {
    text: Option<String>,
    html: Option<String>,
    attachments: Vec<AttachmentMeta>,
}
```

Add the MIME parsing functions:

```rust
/// Parse raw email headers+body into structured text/html/attachments using MIME parsing.
fn parse_mime_body(raw_headers: &str, raw_body: &str) -> ParsedBody {
    let full_message = format!("{}\r\n{}", raw_headers, raw_body);
    let parsed = match mailparse::parse_mail(full_message.as_bytes()) {
        Ok(p) => p,
        Err(_) => {
            return ParsedBody {
                text: Some(raw_body.to_string()),
                html: None,
                attachments: vec![],
            };
        }
    };

    let mut text = None;
    let mut html = None;
    let mut attachments = vec![];

    extract_mime_parts(&parsed, &mut text, &mut html, &mut attachments);

    // Fallback: if no structured parts found, use raw body
    if text.is_none() && html.is_none() {
        text = Some(raw_body.to_string());
    }

    ParsedBody { text, html, attachments }
}

fn extract_mime_parts(
    part: &mailparse::ParsedMail,
    text: &mut Option<String>,
    html: &mut Option<String>,
    attachments: &mut Vec<AttachmentMeta>,
) {
    let content_type = &part.ctype.mimetype;
    let disposition = part.get_content_disposition();

    if disposition.disposition == mailparse::DispositionType::Attachment {
        let filename = disposition
            .params
            .get("filename")
            .cloned()
            .unwrap_or_else(|| "unnamed".to_string());
        let size = part.get_body_raw().map(|b| b.len()).unwrap_or(0);
        attachments.push(AttachmentMeta {
            filename,
            mime_type: content_type.clone(),
            size,
        });
        return;
    }

    if part.subparts.is_empty() {
        match content_type.as_str() {
            "text/plain" if text.is_none() => {
                *text = part.get_body().ok();
            }
            "text/html" if html.is_none() => {
                *html = part.get_body().ok();
            }
            _ => {}
        }
    } else {
        for subpart in &part.subparts {
            extract_mime_parts(subpart, text, html, attachments);
        }
    }
}

/// Extract thread_id from email headers.
/// Priority: References (first ID = thread root) > In-Reply-To > own Message-ID.
fn extract_thread_id(raw_headers: &str, message_id: &Option<String>) -> Option<String> {
    let headers = mailparse::parse_headers(raw_headers.as_bytes())
        .map(|(h, _)| h)
        .unwrap_or_default();

    // Try References header — first Message-ID is the thread root
    if let Some(refs) = headers.get_first_value("References") {
        if let Some(first_ref) = refs.split_whitespace().next() {
            let cleaned = first_ref.trim_matches(|c| c == '<' || c == '>');
            if !cleaned.is_empty() {
                return Some(cleaned.to_string());
            }
        }
    }

    // Fall back to In-Reply-To
    if let Some(in_reply_to) = headers.get_first_value("In-Reply-To") {
        if let Some(first_ref) = in_reply_to.split_whitespace().next() {
            let cleaned = first_ref.trim_matches(|c| c == '<' || c == '>');
            if !cleaned.is_empty() {
                return Some(cleaned.to_string());
            }
        }
    }

    // Standalone — use own message_id
    message_id
        .as_ref()
        .map(|id| id.trim_matches(|c| c == '<' || c == '>').to_string())
}
```

**Step 6: Update `parse_fetch()` to use MIME parsing and thread ID**

In `parse_fetch()`, after the existing `raw_headers` and `body_text` extraction (around line 226), replace the bottom section (lines 228-272) with:

```rust
    // --- MIME parsing ---
    let mime_parsed = match (&raw_headers, &body_text) {
        (Some(h), Some(b)) => parse_mime_body(h, b),
        _ => ParsedBody {
            text: body_text.clone(),
            html: None,
            attachments: vec![],
        },
    };

    // Thread ID extraction
    let thread_id = raw_headers
        .as_ref()
        .and_then(|h| extract_thread_id(h, &message_id));

    // Snippet: first 200 chars of plain text
    let snippet = mime_parsed.text.as_ref().map(|text| {
        text.chars()
            .filter(|c| !c.is_control())
            .take(200)
            .collect::<String>()
    });

    // Attachment metadata as JSON
    let has_attachments = !mime_parsed.attachments.is_empty();
    let attachment_names = if has_attachments {
        serde_json::to_string(&mime_parsed.attachments).ok()
    } else {
        None
    };

    // Check flags
    let is_read = fetch.flags().any(|f| matches!(f, async_imap::types::Flag::Seen));
    let is_starred = fetch.flags().any(|f| matches!(f, async_imap::types::Flag::Flagged));
    let is_draft = fetch.flags().any(|f| matches!(f, async_imap::types::Flag::Draft));

    InsertMessage {
        account_id: account_id.to_string(),
        message_id,
        thread_id,
        folder: "INBOX".to_string(),
        from_address,
        from_name,
        to_addresses,
        cc_addresses,
        bcc_addresses: None,
        subject,
        date,
        snippet,
        body_text: mime_parsed.text,
        body_html: mime_parsed.html,
        is_read,
        is_starred,
        is_draft,
        labels: None,
        uid: fetch.uid.map(|u| u as i64),
        modseq: fetch.modseq.map(|m| m as i64),
        raw_headers,
        has_attachments,
        attachment_names,
        size_bytes: fetch.size.map(|s| s as i64),
    }
```

**Step 7: Run tests to verify they pass**

Run: `cargo test 2>&1`
Expected: All 16 tests pass (10 existing + 6 new).

**Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock src/imap/sync.rs src/models/message.rs
git commit -m "feat(v2): MIME parsing for body/attachments and thread ID extraction"
```

---

### Task 2: MessageDetail Model + Thread Queries

Add a full-detail message struct for the thread view, with get-by-id, list-by-thread, and mark-as-read queries.

**Files:**
- Modify: `src/models/message.rs` (add MessageDetail, get_by_id, list_by_thread, mark_as_read)

**Step 1: Write failing tests**

Add these tests to the existing `mod tests` block in `src/models/message.rs`:

```rust
    #[test]
    fn test_message_detail_get_by_id() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        let mut msg = make_insert_message(&account.id, "INBOX", "Detail Test", false);
        msg.body_text = Some("Full body text here.".to_string());
        msg.body_html = Some("<p>Full body HTML</p>".to_string());
        let id = InsertMessage::insert(&conn, &msg);

        let detail = MessageDetail::get_by_id(&conn, &id);
        assert!(detail.is_some());
        let detail = detail.unwrap();
        assert_eq!(detail.id, id);
        assert_eq!(detail.body_text.as_deref(), Some("Full body text here."));
        assert_eq!(detail.body_html.as_deref(), Some("<p>Full body HTML</p>"));
    }

    #[test]
    fn test_message_detail_list_by_thread() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        // Three messages in same thread, one in different thread
        let thread_id = "thread-abc@example.com";
        for i in 0..3 {
            let mut msg = make_insert_message(&account.id, "INBOX", &format!("Thread msg {i}"), false);
            msg.thread_id = Some(thread_id.to_string());
            msg.message_id = Some(format!("<thread-msg-{i}@example.com>"));
            msg.date = Some(1700000000 + i);
            msg.uid = Some(100 + i);
            InsertMessage::insert(&conn, &msg);
        }

        let mut other = make_insert_message(&account.id, "INBOX", "Other thread", false);
        other.thread_id = Some("other-thread@example.com".to_string());
        other.message_id = Some("<other@example.com>".to_string());
        other.uid = Some(200);
        InsertMessage::insert(&conn, &other);

        let thread = MessageDetail::list_by_thread(&conn, thread_id);
        assert_eq!(thread.len(), 3);
        // Should be ordered by date ASC (chronological)
        assert_eq!(thread[0].subject.as_deref(), Some("Thread msg 0"));
        assert_eq!(thread[2].subject.as_deref(), Some("Thread msg 2"));
    }

    #[test]
    fn test_mark_as_read() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        let msg = make_insert_message(&account.id, "INBOX", "Unread msg", false);
        let id = InsertMessage::insert(&conn, &msg);

        // Verify unread
        let detail = MessageDetail::get_by_id(&conn, &id).unwrap();
        assert!(!detail.is_read);

        // Mark as read
        let result = mark_as_read(&conn, &id);
        assert!(result);

        // Verify read
        let detail = MessageDetail::get_by_id(&conn, &id).unwrap();
        assert!(detail.is_read);
    }
```

**Step 2: Run tests to verify they fail**

Run: `cargo test models::message::tests -- --nocapture 2>&1`
Expected: FAIL — `MessageDetail` and `mark_as_read` don't exist yet.

**Step 3: Implement MessageDetail, list_by_thread, and mark_as_read**

Add `MessageDetail` struct and its methods to `src/models/message.rs`, after the `MessageSummary` impl block:

```rust
/// Full message detail including body, used for thread/detail views.
#[derive(Debug, Clone, Serialize)]
pub struct MessageDetail {
    pub id: String,
    pub account_id: String,
    pub thread_id: Option<String>,
    pub folder: String,
    pub from_address: Option<String>,
    pub from_name: Option<String>,
    pub to_addresses: Option<String>,
    pub cc_addresses: Option<String>,
    pub subject: Option<String>,
    pub snippet: Option<String>,
    pub date: Option<i64>,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub is_read: bool,
    pub is_starred: bool,
    pub has_attachments: bool,
    pub attachments: Vec<AttachmentMeta>,
}

impl MessageDetail {
    pub fn from_row(row: &Row) -> rusqlite::Result<Self> {
        let attachment_json: Option<String> = row.get("attachment_names")?;
        let attachments: Vec<AttachmentMeta> = attachment_json
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        Ok(Self {
            id: row.get("id")?,
            account_id: row.get("account_id")?,
            thread_id: row.get("thread_id")?,
            folder: row.get("folder")?,
            from_address: row.get("from_address")?,
            from_name: row.get("from_name")?,
            to_addresses: row.get("to_addresses")?,
            cc_addresses: row.get("cc_addresses")?,
            subject: row.get("subject")?,
            snippet: row.get("snippet")?,
            date: row.get("date")?,
            body_text: row.get("body_text")?,
            body_html: row.get("body_html")?,
            is_read: row.get("is_read")?,
            is_starred: row.get("is_starred")?,
            has_attachments: row.get("has_attachments")?,
            attachments,
        })
    }

    /// Fetch a single message by its database ID with full body content.
    pub fn get_by_id(conn: &Conn, id: &str) -> Option<Self> {
        conn.query_row(
            "SELECT id, account_id, thread_id, folder, from_address, from_name,
                    to_addresses, cc_addresses, subject, snippet, date,
                    body_text, body_html, is_read, is_starred, has_attachments, attachment_names
             FROM messages WHERE id = ?1 AND is_deleted = 0",
            rusqlite::params![id],
            Self::from_row,
        )
        .ok()
    }

    /// Fetch all messages in a thread, ordered chronologically (ASC).
    pub fn list_by_thread(conn: &Conn, thread_id: &str) -> Vec<Self> {
        let mut stmt = conn
            .prepare(
                "SELECT id, account_id, thread_id, folder, from_address, from_name,
                        to_addresses, cc_addresses, subject, snippet, date,
                        body_text, body_html, is_read, is_starred, has_attachments, attachment_names
                 FROM messages WHERE thread_id = ?1 AND is_deleted = 0
                 ORDER BY date ASC",
            )
            .expect("failed to prepare list_by_thread");

        stmt.query_map(rusqlite::params![thread_id], Self::from_row)
            .expect("failed to query thread messages")
            .filter_map(|r| r.ok())
            .collect()
    }
}

/// Mark a message as read. Returns true if a row was updated.
pub fn mark_as_read(conn: &Conn, id: &str) -> bool {
    conn.execute(
        "UPDATE messages SET is_read = 1, updated_at = datetime('now') WHERE id = ?1",
        rusqlite::params![id],
    )
    .map(|rows| rows > 0)
    .unwrap_or(false)
}
```

**Step 4: Run tests to verify they pass**

Run: `cargo test 2>&1`
Expected: All 19 tests pass (10 original + 6 MIME + 3 new).

**Step 5: Commit**

```bash
git add src/models/message.rs
git commit -m "feat(v2): MessageDetail model with thread queries and mark-as-read"
```

---

### Task 3: Thread API Endpoints

Add REST endpoints for message detail, thread view, and mark-as-read. Wire routes in main.rs.

**Files:**
- Create: `src/api/threads.rs`
- Modify: `src/api/messages.rs` (add get_message, mark_message_read handlers)
- Modify: `src/api/mod.rs` (add threads module)
- Modify: `src/main.rs` (wire new routes)

**Step 1: Create src/api/threads.rs**

```rust
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use std::collections::HashSet;
use std::sync::Arc;

use crate::models::message::MessageDetail;
use crate::AppState;

#[derive(Debug, Serialize)]
pub struct Participant {
    pub email: String,
    pub name: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ThreadResponse {
    pub thread_id: String,
    pub subject: Option<String>,
    pub participants: Vec<Participant>,
    pub message_count: usize,
    pub messages: Vec<MessageDetail>,
}

pub async fn get_thread(
    State(state): State<Arc<AppState>>,
    Path(thread_id): Path<String>,
) -> Result<Json<ThreadResponse>, StatusCode> {
    let conn = state
        .db
        .get()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let messages = MessageDetail::list_by_thread(&conn, &thread_id);

    if messages.is_empty() {
        return Err(StatusCode::NOT_FOUND);
    }

    let subject = messages[0].subject.clone();

    let mut seen = HashSet::new();
    let mut participants = Vec::new();
    for msg in &messages {
        if let Some(ref email) = msg.from_address {
            if seen.insert(email.clone()) {
                participants.push(Participant {
                    email: email.clone(),
                    name: msg.from_name.clone(),
                });
            }
        }
    }

    Ok(Json(ThreadResponse {
        thread_id,
        subject,
        participants,
        message_count: messages.len(),
        messages,
    }))
}
```

**Step 2: Add get_message and mark_message_read to src/api/messages.rs**

Add these imports at the top:

```rust
use axum::extract::Path;
use crate::models::message::{self, MessageDetail, MessageSummary};
```

(Replace the existing `use crate::models::message::{self, MessageSummary};` line.)

Add these handlers after the existing `list_messages` function:

```rust
pub async fn get_message(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<MessageDetail>, StatusCode> {
    let conn = state
        .db
        .get()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    MessageDetail::get_by_id(&conn, &id)
        .map(Json)
        .ok_or(StatusCode::NOT_FOUND)
}

pub async fn mark_message_read(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, StatusCode> {
    let conn = state
        .db
        .get()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if message::mark_as_read(&conn, &id) {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}
```

**Step 3: Add threads module to src/api/mod.rs**

Current content:
```rust
pub mod accounts;
pub mod config;
pub mod health;
pub mod messages;
```

Add:
```rust
pub mod threads;
```

**Step 4: Wire new routes in src/main.rs**

In the `api_routes` builder (around line 44-51), add these routes. The `{id}` route for messages needs to be separate from the existing `/messages` route:

```rust
.route("/messages/{id}", get(api::messages::get_message))
.route("/messages/{id}/read", put(api::messages::mark_message_read))
.route("/threads/{id}", get(api::threads::get_thread))
```

Also add `put` to the `use axum::routing::{get, put};` import at the top of main.rs (it's already imported for the config/theme route).

**Step 5: Build and verify**

Run: `cargo build 2>&1`
Expected: Compiles with only the expected dead_code warnings.

Run: `cargo test 2>&1`
Expected: All 19 tests pass.

**Step 6: Commit**

```bash
git add src/api/threads.rs src/api/messages.rs src/api/mod.rs src/main.rs
git commit -m "feat(v2): thread and message detail API endpoints"
```

---

### Task 4: Frontend — EmailBody Sanitized Renderer

Install DOMPurify and create a sandboxed iframe component for safely rendering email HTML.

**Files:**
- Modify: `web/package.json` (add dompurify)
- Create: `web/src/components/thread/EmailBody.svelte`

**Step 1: Install DOMPurify**

```bash
cd web && npm install dompurify && cd ..
```

**Step 2: Create EmailBody.svelte**

Create directory `web/src/components/thread/` and file `EmailBody.svelte`:

```svelte
<script lang="ts">
  import DOMPurify from 'dompurify';

  let { html, text }: { html?: string | null; text?: string | null } = $props();

  let iframeEl: HTMLIFrameElement;

  function getSanitizedContent(): string {
    if (html) {
      return DOMPurify.sanitize(html, {
        ALLOWED_TAGS: [
          'p', 'br', 'b', 'i', 'u', 'strong', 'em', 'a', 'ul', 'ol', 'li',
          'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'blockquote', 'pre', 'code',
          'table', 'thead', 'tbody', 'tr', 'th', 'td', 'div', 'span', 'img',
          'hr', 'sub', 'sup',
        ],
        ALLOWED_ATTR: [
          'href', 'src', 'alt', 'style', 'target', 'width', 'height',
          'colspan', 'rowspan',
        ],
        ALLOW_DATA_ATTR: false,
      });
    }
    if (text) {
      const escaped = text
        .replace(/&/g, '&amp;')
        .replace(/</g, '&lt;')
        .replace(/>/g, '&gt;');
      return `<pre style="white-space:pre-wrap;word-wrap:break-word;font-family:inherit;">${escaped}</pre>`;
    }
    return '<p style="color:#999;">No content</p>';
  }

  $effect(() => {
    if (iframeEl) {
      const doc = iframeEl.contentDocument;
      if (doc) {
        doc.open();
        doc.write(`<!DOCTYPE html>
<html><head><style>
body{margin:0;padding:8px;font-family:-apple-system,system-ui,sans-serif;font-size:14px;line-height:1.6;color:#333;}
a{color:#2563eb;}
img{max-width:100%;height:auto;}
blockquote{margin:8px 0;padding-left:12px;border-left:3px solid #ddd;color:#666;}
table{border-collapse:collapse;}
td,th{padding:4px 8px;}
</style></head><body>${getSanitizedContent()}</body></html>`);
        doc.close();
        // Auto-resize iframe to content height
        setTimeout(() => {
          if (iframeEl && doc.body) {
            iframeEl.style.height = doc.body.scrollHeight + 'px';
          }
        }, 50);
      }
    }
  });
</script>

<iframe
  bind:this={iframeEl}
  sandbox="allow-same-origin"
  class="w-full border-0 min-h-[100px]"
  title="Email content"
></iframe>
```

**Step 3: Build to verify**

Run: `cd web && npm run build 2>&1 && cd ..`
Expected: Build succeeds (EmailBody isn't imported anywhere yet, but the module should be parseable).

**Step 4: Commit**

```bash
git add web/package.json web/package-lock.json web/src/components/thread/EmailBody.svelte
git commit -m "feat(v2): EmailBody component with DOMPurify and sandboxed iframe"
```

---

### Task 5: Frontend — ThreadView Page + MessageCard

Create the thread view page with thread header (U13), message list (U15), message body rendering (U16), attachment list (U19), and reply button stubs (U22).

**Files:**
- Create: `web/src/components/thread/MessageCard.svelte`
- Create: `web/src/pages/ThreadView.svelte`

**Step 1: Create MessageCard.svelte**

```svelte
<script lang="ts">
  import EmailBody from './EmailBody.svelte';

  let { message }: { message: any } = $props();
  let expanded = $state(true);

  function formatDate(ts: number): string {
    return new Date(ts * 1000).toLocaleString([], {
      month: 'short',
      day: 'numeric',
      year: 'numeric',
      hour: 'numeric',
      minute: '2-digit',
    });
  }

  function formatSize(bytes: number): string {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  function parseAddresses(json: string | null): string {
    if (!json) return '';
    try {
      return JSON.parse(json).join(', ');
    } catch {
      return json;
    }
  }
</script>

<div class="border border-gray-200 dark:border-gray-700 rounded-lg bg-white dark:bg-gray-900">
  <button
    class="w-full flex items-center justify-between p-4 text-left hover:bg-gray-50 dark:hover:bg-gray-800/50 transition-colors"
    onclick={() => (expanded = !expanded)}
  >
    <div class="flex items-center gap-3 min-w-0">
      <span class="font-medium text-sm truncate">
        {message.from_name || message.from_address || 'Unknown'}
      </span>
      <span class="text-xs text-gray-400 flex-shrink-0">
        {message.date ? formatDate(message.date) : ''}
      </span>
    </div>
    <span class="text-gray-400 ml-2 flex-shrink-0">{expanded ? '\u25BE' : '\u25B8'}</span>
  </button>

  {#if expanded}
    <div class="px-4 pb-4">
      <!-- Recipients -->
      <div class="text-xs text-gray-500 dark:text-gray-400 mb-3 space-y-0.5">
        {#if message.to_addresses}
          <div>To: {parseAddresses(message.to_addresses)}</div>
        {/if}
        {#if message.cc_addresses}
          <div>Cc: {parseAddresses(message.cc_addresses)}</div>
        {/if}
      </div>

      <!-- Body (U16) -->
      <EmailBody html={message.body_html} text={message.body_text} />

      <!-- Attachments (U19) -->
      {#if message.attachments && message.attachments.length > 0}
        <div class="mt-4 pt-3 border-t border-gray-100 dark:border-gray-800">
          <p class="text-xs font-medium text-gray-600 dark:text-gray-400 mb-2">
            Attachments ({message.attachments.length})
          </p>
          <div class="flex flex-wrap gap-2">
            {#each message.attachments as att}
              <div
                class="flex items-center gap-1.5 px-3 py-1.5 bg-gray-50 dark:bg-gray-800 rounded text-xs text-gray-600 dark:text-gray-400"
              >
                <span>{'\u{1F4CE}'}</span>
                <span class="truncate max-w-[200px]">{att.filename}</span>
                <span class="text-gray-400">({formatSize(att.size)})</span>
              </div>
            {/each}
          </div>
        </div>
      {/if}
    </div>
  {/if}
</div>
```

**Step 2: Create ThreadView.svelte**

```svelte
<script lang="ts">
  import { api } from '../lib/api';
  import { wsClient } from '../lib/ws';
  import { push } from 'svelte-spa-router';
  import MessageCard from '../components/thread/MessageCard.svelte';

  let { params }: { params: { id: string } } = $props();

  let thread = $state<any>(null);
  let loading = $state(true);
  let error = $state('');

  async function loadThread() {
    loading = true;
    error = '';
    try {
      thread = await api.threads.get(params.id);
      // Auto mark-as-read for unread messages
      for (const msg of thread.messages) {
        if (!msg.is_read) {
          await api.messages.markRead(msg.id);
          msg.is_read = true;
        }
      }
    } catch (e: any) {
      error = e.message || 'Failed to load thread';
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    if (params.id) {
      loadThread();
    }

    const off = wsClient.on('NewEmail', () => {
      if (thread) loadThread();
    });

    return () => off();
  });
</script>

<div class="h-full flex flex-col">
  <!-- Thread header (U13) -->
  <div class="px-4 py-3 border-b border-gray-200 dark:border-gray-700 flex items-center gap-3">
    <button
      class="p-1 text-gray-500 hover:text-gray-700 dark:hover:text-gray-300 transition-colors"
      onclick={() => push('/')}
      title="Back to inbox"
    >
      &larr;
    </button>
    {#if thread}
      <div class="flex-1 min-w-0">
        <h2 class="text-lg font-semibold truncate">{thread.subject || '(no subject)'}</h2>
        <p class="text-xs text-gray-500 truncate">
          {thread.participants.map((p: any) => p.name || p.email).join(', ')}
          &middot; {thread.message_count} message{thread.message_count === 1 ? '' : 's'}
        </p>
      </div>
    {/if}
  </div>

  <!-- Message list (U15) -->
  <div class="flex-1 overflow-y-auto p-4 space-y-3">
    {#if loading}
      <div class="flex items-center justify-center py-16">
        <div class="w-8 h-8 border-4 border-blue-200 border-t-blue-600 rounded-full animate-spin"></div>
      </div>
    {:else if error}
      <div class="text-center py-16">
        <p class="text-red-500 dark:text-red-400 mb-4">{error}</p>
        <button
          class="px-4 py-2 bg-blue-600 hover:bg-blue-700 text-white rounded-lg text-sm font-medium"
          onclick={loadThread}
        >
          Retry
        </button>
      </div>
    {:else if thread}
      {#each thread.messages as message (message.id)}
        <MessageCard {message} />
      {/each}
    {/if}
  </div>

  <!-- Reply / Forward stubs (U22) — disabled until V3 -->
  {#if thread && !loading}
    <div class="px-4 py-3 border-t border-gray-200 dark:border-gray-700 flex gap-2">
      <button
        disabled
        class="px-4 py-2 text-sm bg-gray-100 dark:bg-gray-800 text-gray-400 rounded-lg cursor-not-allowed"
        title="Coming in V3"
      >
        Reply
      </button>
      <button
        disabled
        class="px-4 py-2 text-sm bg-gray-100 dark:bg-gray-800 text-gray-400 rounded-lg cursor-not-allowed"
        title="Coming in V3"
      >
        Reply All
      </button>
      <button
        disabled
        class="px-4 py-2 text-sm bg-gray-100 dark:bg-gray-800 text-gray-400 rounded-lg cursor-not-allowed"
        title="Coming in V3"
      >
        Forward
      </button>
    </div>
  {/if}
</div>
```

**Step 3: Build to verify**

Run: `cd web && npm run build 2>&1 && cd ..`
Expected: Build succeeds (components aren't routed yet but should compile).

**Step 4: Commit**

```bash
git add web/src/components/thread/MessageCard.svelte web/src/pages/ThreadView.svelte
git commit -m "feat(v2): ThreadView page with MessageCard, attachments, and reply stubs"
```

---

### Task 6: Frontend — Routing + Inbox Click-Through + API Client

Wire the thread route, update inbox to navigate on click, add thread/message API methods.

**Files:**
- Modify: `web/src/lib/api.ts` (add threads.get, messages.get, messages.markRead)
- Modify: `web/src/App.svelte` (add /thread/:id route)
- Modify: `web/src/pages/Inbox.svelte` (wire row click to navigate)

**Step 1: Update api.ts**

Add `threads` and extend `messages` in the exported `api` object:

```typescript
export const api = {
  health: () => request<{ status: string; version: string }>('/api/health'),
  accounts: {
    list: () => request<any[]>('/api/accounts'),
    get: (id: string) => request<any>(`/api/accounts/${id}`),
    create: (data: any) => request<any>('/api/accounts', { method: 'POST', body: JSON.stringify(data) }),
  },
  messages: {
    list: (params?: { account_id?: string; folder?: string; limit?: number; offset?: number }) => {
      const query = new URLSearchParams();
      if (params?.account_id) query.set('account_id', params.account_id);
      if (params?.folder) query.set('folder', params.folder);
      if (params?.limit) query.set('limit', String(params.limit));
      if (params?.offset) query.set('offset', String(params.offset));
      return request<{ messages: any[]; unread_count: number; total: number }>(`/api/messages?${query}`);
    },
    get: (id: string) => request<any>(`/api/messages/${id}`),
    markRead: (id: string) => fetch(`/api/messages/${id}/read`, { method: 'PUT' }),
  },
  threads: {
    get: (id: string) => request<any>(`/api/threads/${id}`),
  },
  config: {
    get: () => request<{ theme: string }>('/api/config'),
    setTheme: (theme: string) => request<void>('/api/config/theme', { method: 'PUT', body: JSON.stringify({ theme }) }),
  },
  auth: {
    startOAuth: (provider: string) => request<{ url: string }>(`/api/auth/oauth/${provider}`),
  },
};
```

**Step 2: Update App.svelte — add ThreadView route**

Add import and route entry:

```svelte
<script lang="ts">
  import './app.css';
  import Router from 'svelte-spa-router';
  import AppShell from './components/AppShell.svelte';
  import Inbox from './pages/Inbox.svelte';
  import ThreadView from './pages/ThreadView.svelte';
  import AccountSetup from './pages/AccountSetup.svelte';
  import Settings from './pages/Settings.svelte';

  const routes = {
    '/': Inbox,
    '/thread/:id': ThreadView,
    '/setup': AccountSetup,
    '/setup/*': AccountSetup,
    '/settings': Settings,
  };
</script>

<AppShell>
  <Router {routes} />
</AppShell>
```

**Step 3: Update Inbox.svelte — wire click to navigate**

Add `push` import and update `handleMessageClick`:

Replace the import section at the top:
```typescript
  import { push } from 'svelte-spa-router';
```
(Add this after the existing imports.)

Replace the `handleMessageClick` function:
```typescript
  function handleMessageClick(id: string) {
    const msg = messages.find((m) => m.id === id);
    const threadId = msg?.thread_id || id;
    push(`/thread/${threadId}`);
  }
```

**Step 4: Build and verify the full frontend**

Run: `cd web && npm run build 2>&1 && cd ..`
Expected: Build succeeds.

**Step 5: Commit**

```bash
git add web/src/lib/api.ts web/src/App.svelte web/src/pages/Inbox.svelte
git commit -m "feat(v2): wire thread routing, inbox click-through, and API client"
```

---

### Task 7: Integration + Smoke Test

Build everything, run all tests, and verify the full read flow with a running server.

**Files:**
- No new files

**Step 1: Build frontend**

Run: `cd web && npm run build 2>&1 && cd ..`
Expected: Build succeeds.

**Step 2: Build backend**

Run: `cargo build 2>&1`
Expected: Compiles with only expected dead_code warnings.

**Step 3: Run all tests**

Run: `cargo test 2>&1`
Expected: All 19 tests pass (10 V1 + 6 MIME/threading + 3 model).

**Step 4: Smoke test — start server and verify endpoints**

Find a free port, start the server, and test all new V2 endpoints:

```bash
# Start server
PORT=<free-port> cargo run &

# Test V1 endpoints still work
curl -s http://localhost:<port>/api/health
# Expected: {"status":"ok","version":"0.1.0"}

curl -s http://localhost:<port>/api/accounts
# Expected: []

# Test V2 endpoints
curl -s http://localhost:<port>/api/messages/nonexistent
# Expected: HTTP 404 (empty response)

curl -s http://localhost:<port>/api/threads/nonexistent
# Expected: HTTP 404 (empty response)

# Test SPA serves index.html
curl -s http://localhost:<port>/ | head -1
# Expected: <!doctype html>

# Kill server
kill %1
```

**Step 5: Commit (if any adjustments were needed)**

```bash
git add -A
git commit -m "feat(v2): integration verification"
```

---

## Summary

| Task | What | Tests Added | Commit |
|------|------|-------------|--------|
| 1 | MIME parsing + thread ID in sync | 6 (3 MIME + 3 threading) | `feat(v2): MIME parsing...` |
| 2 | MessageDetail model + queries | 3 (get_by_id, list_by_thread, mark_read) | `feat(v2): MessageDetail model...` |
| 3 | Thread API endpoints + routes | 0 (tested via smoke) | `feat(v2): thread and message detail API...` |
| 4 | EmailBody sanitized renderer | 0 (visual) | `feat(v2): EmailBody component...` |
| 5 | ThreadView + MessageCard | 0 (visual) | `feat(v2): ThreadView page...` |
| 6 | Routing + inbox click-through + API | 0 (integration) | `feat(v2): wire thread routing...` |
| 7 | Integration smoke test | 0 | `feat(v2): integration verification` |

**Total new tests:** 9 (running total: 19)
**New dependencies:** mailparse 0.15 (Rust), dompurify 3.x (npm)
**New files:** 4 (threads.rs, EmailBody.svelte, MessageCard.svelte, ThreadView.svelte)
**Modified files:** 7 (Cargo.toml, message.rs, sync.rs, messages.rs, mod.rs, main.rs, api.ts, App.svelte, Inbox.svelte)
