# V1: Foundation Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Stand up the complete Iris infrastructure — Docker deployment, Rust backend with REST + WebSocket, SQLite schema, IMAP sync with OAuth2, and a Svelte 5 SPA with account setup and basic inbox list.

**Architecture:** Rust (Axum) HTTP server serves a Svelte SPA and REST API on a single port. WebSocket provides real-time push for new mail and sync status. SQLite stores all data locally with FTS5 for future search. IMAP engine handles email sync per account. Docker Compose bundles iris-server + Ollama.

**Tech Stack:**
- **Backend:** Rust (Axum, tokio, rusqlite bundled, async-imap, oauth2), SQLite + FTS5
- **Frontend:** Svelte 5 + TypeScript + Vite + Tailwind CSS 4 + svelte-routing
- **Deployment:** Docker Compose (iris-server + Ollama)
- **Testing:** `cargo test` (Rust), `vitest` + `@testing-library/svelte` (frontend)

**Demo target:** `docker compose up` → open browser → add Gmail via OAuth → emails sync and appear in inbox → new mail pushes live via WebSocket.

---

## Project Structure

```
iris/
├── Cargo.toml
├── Cargo.lock
├── docker-compose.yml
├── Dockerfile
├── .env.example
├── src/                        # Rust backend
│   ├── main.rs
│   ├── config.rs
│   ├── db/
│   │   ├── mod.rs
│   │   └── migrations.rs
│   ├── api/
│   │   ├── mod.rs
│   │   ├── health.rs
│   │   ├── accounts.rs
│   │   └── messages.rs
│   ├── auth/
│   │   ├── mod.rs
│   │   └── oauth.rs
│   ├── imap/
│   │   ├── mod.rs
│   │   ├── connection.rs
│   │   ├── sync.rs
│   │   └── idle.rs
│   ├── ws/
│   │   ├── mod.rs
│   │   └── hub.rs
│   └── models/
│       ├── mod.rs
│       ├── account.rs
│       └── message.rs
├── migrations/
│   └── 001_initial.sql
├── web/                        # Svelte 5 frontend
│   ├── package.json
│   ├── vite.config.ts
│   ├── svelte.config.js
│   ├── tsconfig.json
│   ├── index.html
│   └── src/
│       ├── main.ts
│       ├── App.svelte
│       ├── app.css
│       ├── lib/
│       │   ├── api.ts
│       │   ├── ws.ts
│       │   └── stores/
│       │       ├── accounts.svelte.ts
│       │       ├── messages.svelte.ts
│       │       └── theme.svelte.ts
│       ├── components/
│       │   ├── AppShell.svelte
│       │   ├── Sidebar.svelte
│       │   ├── Header.svelte
│       │   └── inbox/
│       │       ├── MessageList.svelte
│       │       ├── MessageRow.svelte
│       │       ├── UnreadBadge.svelte
│       │       └── SyncStatus.svelte
│       └── pages/
│           ├── Inbox.svelte
│           ├── AccountSetup.svelte
│           └── Settings.svelte
├── tests/                      # Rust integration tests
│   ├── health_test.rs
│   ├── accounts_test.rs
│   └── messages_test.rs
└── docs/
```

---

## Prerequisites

Before starting implementation:

1. **Gmail OAuth2 credentials** (for testing):
   - Go to Google Cloud Console → Create project → Enable Gmail API
   - Create OAuth2 credentials (Web application)
   - Authorized redirect URI: `http://localhost:3000/auth/callback`
   - Save client_id and client_secret for `.env`

2. **Rust toolchain:** `rustup` with stable channel
3. **Node.js:** v20+ with npm
4. **Docker:** OrbStack (already installed per env context)

---

## Task 1: Rust Project Scaffolding

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`
- Create: `src/config.rs`
- Create: `.env.example`
- Create: `.gitignore`

**Step 1: Initialize Cargo project**

Run: `cargo init --name iris-server .`

**Step 2: Configure dependencies in Cargo.toml**

```toml
[package]
name = "iris-server"
version = "0.1.0"
edition = "2024"

[dependencies]
# Web framework
axum = { version = "0.8", features = ["ws", "macros"] }
axum-extra = { version = "0.10", features = ["typed-header"] }
tower-http = { version = "0.6", features = ["cors", "fs", "compression-gzip"] }
tokio = { version = "1", features = ["full"] }

# Database
rusqlite = { version = "0.32", features = ["bundled", "column_decltype"] }
r2d2 = "0.8"
r2d2_sqlite = "0.25"

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# Auth
oauth2 = "5"
reqwest = { version = "0.12", features = ["json", "rustls-tls"], default-features = false }

# IMAP
async-imap = "0.10"
async-native-tls = "0.5"

# Utilities
uuid = { version = "1", features = ["v4", "serde"] }
chrono = { version = "0.4", features = ["serde"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
dotenvy = "0.15"
thiserror = "2"
tokio-stream = "0.1"
```

**Step 3: Write config module**

```rust
// src/config.rs
use std::env;

#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub database_url: String,
    pub ollama_url: String,
    pub gmail_client_id: Option<String>,
    pub gmail_client_secret: Option<String>,
    pub outlook_client_id: Option<String>,
    pub outlook_client_secret: Option<String>,
    pub public_url: String,
}

impl Config {
    pub fn from_env() -> Self {
        Self {
            port: env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(3000),
            database_url: env::var("DATABASE_URL").unwrap_or_else(|_| "./data/iris.db".into()),
            ollama_url: env::var("OLLAMA_URL").unwrap_or_else(|_| "http://localhost:11434".into()),
            gmail_client_id: env::var("GMAIL_CLIENT_ID").ok(),
            gmail_client_secret: env::var("GMAIL_CLIENT_SECRET").ok(),
            outlook_client_id: env::var("OUTLOOK_CLIENT_ID").ok(),
            outlook_client_secret: env::var("OUTLOOK_CLIENT_SECRET").ok(),
            public_url: env::var("PUBLIC_URL").unwrap_or_else(|_| "http://localhost:3000".into()),
        }
    }
}
```

**Step 4: Write minimal main.rs**

```rust
// src/main.rs
mod config;

use config::Config;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt().with_env_filter("iris_server=debug,info").init();

    let config = Config::from_env();
    tracing::info!("Iris starting on port {}", config.port);
}
```

**Step 5: Write .env.example**

```env
PORT=3000
DATABASE_URL=./data/iris.db
OLLAMA_URL=http://localhost:11434
PUBLIC_URL=http://localhost:3000

# Gmail OAuth2 (optional — needed for Gmail accounts)
GMAIL_CLIENT_ID=
GMAIL_CLIENT_SECRET=

# Outlook OAuth2 (optional — needed for Outlook accounts)
OUTLOOK_CLIENT_ID=
OUTLOOK_CLIENT_SECRET=
```

**Step 6: Update .gitignore**

```gitignore
/target
/data
/web/node_modules
/web/dist
.env
*.db
*.db-journal
*.db-wal
```

**Step 7: Verify compilation**

Run: `cargo build`
Expected: Compiles successfully (warnings OK at this stage).

**Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock src/ .env.example .gitignore
git commit -m "feat(v1): scaffold Rust project with Axum dependencies"
```

---

## Task 2: Docker Compose Setup

**Files:**
- Create: `Dockerfile`
- Create: `docker-compose.yml`

**Step 1: Write multi-stage Dockerfile**

```dockerfile
# Dockerfile
FROM rust:1.85-slim AS builder
WORKDIR /app
RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
RUN cargo build --release

FROM node:20-slim AS frontend
WORKDIR /app
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ .
RUN npm run build

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y ca-certificates && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/iris-server .
COPY --from=frontend /app/dist ./web/dist
COPY migrations/ migrations/
ENV PORT=3000
EXPOSE 3000
CMD ["./iris-server"]
```

**Step 2: Write docker-compose.yml**

```yaml
# docker-compose.yml
services:
  iris:
    build: .
    ports:
      - "${PORT:-3000}:3000"
    volumes:
      - iris-data:/app/data
    environment:
      - DATABASE_URL=/app/data/iris.db
      - OLLAMA_URL=http://ollama:11434
      - PORT=3000
    env_file:
      - .env
    depends_on:
      ollama:
        condition: service_started
    restart: unless-stopped

  ollama:
    image: ollama/ollama:latest
    volumes:
      - ollama-data:/root/.ollama
    restart: unless-stopped

volumes:
  iris-data:
  ollama-data:
```

**Step 3: Verify compose config parses**

Run: `docker compose config --quiet`
Expected: No errors.

**Step 4: Commit**

```bash
git add Dockerfile docker-compose.yml
git commit -m "feat(v1): add Docker Compose with iris-server + Ollama"
```

---

## Task 3: SQLite Database + Schema

**Files:**
- Create: `src/db/mod.rs`
- Create: `src/db/migrations.rs`
- Create: `migrations/001_initial.sql`

**Step 1: Write the migration SQL**

This includes ALL V1 tables plus nullable AI columns (per V1 key decision: "include nullable AI columns from day one"):

```sql
-- migrations/001_initial.sql

-- Account configurations (S11)
CREATE TABLE IF NOT EXISTS accounts (
    id TEXT PRIMARY KEY,
    provider TEXT NOT NULL,          -- 'gmail', 'outlook', 'yahoo', 'fastmail', 'imap'
    email TEXT NOT NULL UNIQUE,
    display_name TEXT,
    -- OAuth2 tokens (null for password auth)
    access_token TEXT,
    refresh_token TEXT,
    token_expires_at INTEGER,        -- unix timestamp
    -- IMAP/SMTP config (for manual setup)
    imap_host TEXT,
    imap_port INTEGER DEFAULT 993,
    smtp_host TEXT,
    smtp_port INTEGER DEFAULT 587,
    username TEXT,
    password_encrypted TEXT,
    -- Sync state
    last_sync_at INTEGER,
    sync_status TEXT DEFAULT 'pending', -- pending, syncing, idle, error
    sync_error TEXT,
    -- Metadata
    is_active INTEGER DEFAULT 1,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

-- Core email store (S5) — nullable AI columns for V6+
CREATE TABLE IF NOT EXISTS messages (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id),
    message_id TEXT,                 -- RFC Message-ID header
    thread_id TEXT,                  -- grouped by References/In-Reply-To
    folder TEXT NOT NULL DEFAULT 'INBOX',
    -- Headers
    from_address TEXT,
    from_name TEXT,
    to_addresses TEXT,               -- JSON array
    cc_addresses TEXT,               -- JSON array
    bcc_addresses TEXT,              -- JSON array
    subject TEXT,
    date INTEGER,                    -- unix timestamp from Date header
    -- Body
    snippet TEXT,                    -- first ~200 chars plaintext
    body_text TEXT,                  -- plaintext body
    body_html TEXT,                  -- HTML body
    -- Flags
    is_read INTEGER DEFAULT 0,
    is_starred INTEGER DEFAULT 0,
    is_draft INTEGER DEFAULT 0,
    is_deleted INTEGER DEFAULT 0,
    labels TEXT,                     -- JSON array of label strings
    -- IMAP state
    uid INTEGER,                     -- IMAP UID
    modseq INTEGER,                  -- IMAP MODSEQ for sync
    raw_headers TEXT,                -- full headers for auth parsing (V9)
    -- AI metadata (nullable — populated in V6)
    ai_intent TEXT,                  -- ACTION_REQUEST, INFORMATIONAL, etc.
    ai_priority_score REAL,          -- 0.0-1.0 Eisenhower score
    ai_priority_label TEXT,          -- urgent, high, normal, low
    ai_category TEXT,                -- dynamic AI category
    ai_entities TEXT,                -- JSON: extracted people, dates, amounts
    ai_deadline TEXT,                -- extracted deadline ISO string
    ai_summary TEXT,                 -- thread summary (V7, cached)
    -- Metadata
    has_attachments INTEGER DEFAULT 0,
    attachment_names TEXT,           -- JSON array of filenames
    size_bytes INTEGER,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

CREATE INDEX IF NOT EXISTS idx_messages_account ON messages(account_id);
CREATE INDEX IF NOT EXISTS idx_messages_thread ON messages(thread_id);
CREATE INDEX IF NOT EXISTS idx_messages_folder ON messages(account_id, folder);
CREATE INDEX IF NOT EXISTS idx_messages_date ON messages(date DESC);
CREATE INDEX IF NOT EXISTS idx_messages_uid ON messages(account_id, folder, uid);

-- FTS5 full-text index (S10) — populated during sync, searched in V5
CREATE VIRTUAL TABLE IF NOT EXISTS fts_messages USING fts5(
    message_id,
    subject,
    body_text,
    from_address,
    from_name,
    content=messages,
    content_rowid=rowid,
    tokenize='porter unicode61'
);

-- Triggers to keep FTS5 in sync
CREATE TRIGGER IF NOT EXISTS messages_ai AFTER INSERT ON messages BEGIN
    INSERT INTO fts_messages(rowid, message_id, subject, body_text, from_address, from_name)
    VALUES (new.rowid, new.message_id, new.subject, new.body_text, new.from_address, new.from_name);
END;

CREATE TRIGGER IF NOT EXISTS messages_ad AFTER DELETE ON messages BEGIN
    INSERT INTO fts_messages(fts_messages, rowid, message_id, subject, body_text, from_address, from_name)
    VALUES ('delete', old.rowid, old.message_id, old.subject, old.body_text, old.from_address, old.from_name);
END;

CREATE TRIGGER IF NOT EXISTS messages_au AFTER UPDATE ON messages BEGIN
    INSERT INTO fts_messages(fts_messages, rowid, message_id, subject, body_text, from_address, from_name)
    VALUES ('delete', old.rowid, old.message_id, old.subject, old.body_text, old.from_address, old.from_name);
    INSERT INTO fts_messages(rowid, message_id, subject, body_text, from_address, from_name)
    VALUES (new.rowid, new.message_id, new.subject, new.body_text, new.from_address, new.from_name);
END;

-- App config (S12) — key-value store for theme (S4) and future settings
CREATE TABLE IF NOT EXISTS config (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
);

-- Default theme
INSERT OR IGNORE INTO config (key, value) VALUES ('theme', 'system');

-- Schema version tracking
CREATE TABLE IF NOT EXISTS schema_version (
    version INTEGER PRIMARY KEY,
    applied_at INTEGER NOT NULL DEFAULT (unixepoch())
);

INSERT OR IGNORE INTO schema_version (version) VALUES (1);
```

**Step 2: Write database module**

```rust
// src/db/mod.rs
pub mod migrations;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;
use std::path::Path;

pub type DbPool = Pool<SqliteConnectionManager>;

pub fn create_pool(database_url: &str) -> Result<DbPool, Box<dyn std::error::Error>> {
    // Ensure parent directory exists
    if let Some(parent) = Path::new(database_url).parent() {
        std::fs::create_dir_all(parent)?;
    }

    let manager = SqliteConnectionManager::file(database_url);
    let pool = Pool::builder().max_size(10).build(manager)?;

    // Configure SQLite pragmas on each connection
    let conn = pool.get()?;
    configure_connection(&conn)?;

    Ok(pool)
}

fn configure_connection(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;"
    )?;
    Ok(())
}

#[cfg(test)]
pub fn create_test_pool() -> DbPool {
    let manager = SqliteConnectionManager::memory();
    let pool = Pool::builder().max_size(1).build(manager).unwrap();
    let conn = pool.get().unwrap();
    configure_connection(&conn).unwrap();
    migrations::run(&conn).unwrap();
    pool
}
```

```rust
// src/db/migrations.rs
use rusqlite::Connection;

const MIGRATION_001: &str = include_str!("../../migrations/001_initial.sql");

pub fn run(conn: &Connection) -> Result<(), rusqlite::Error> {
    let current_version: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_version",
            [],
            |row| row.get(0),
        )
        .unwrap_or(0);

    if current_version < 1 {
        conn.execute_batch(MIGRATION_001)?;
        tracing::info!("Applied migration 001_initial");
    }

    Ok(())
}
```

**Step 3: Write test for database creation**

Add to `src/db/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_pool_and_run_migrations() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        // Verify tables exist
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(tables.contains(&"accounts".to_string()));
        assert!(tables.contains(&"messages".to_string()));
        assert!(tables.contains(&"config".to_string()));
        assert!(tables.contains(&"schema_version".to_string()));
    }

    #[test]
    fn test_default_theme_is_system() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let theme: String = conn
            .query_row("SELECT value FROM config WHERE key = 'theme'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(theme, "system");
    }

    #[test]
    fn test_fts5_index_exists() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        // FTS5 tables show up as virtual tables
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='fts_messages'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
}
```

**Step 4: Run tests**

Run: `cargo test db`
Expected: All 3 tests pass.

**Step 5: Commit**

```bash
git add src/db/ migrations/
git commit -m "feat(v1): SQLite schema with accounts, messages, FTS5, and config"
```

---

## Task 4: Models

**Files:**
- Create: `src/models/mod.rs`
- Create: `src/models/account.rs`
- Create: `src/models/message.rs`

**Step 1: Write Account model with DB operations**

```rust
// src/models/mod.rs
pub mod account;
pub mod message;
```

```rust
// src/models/account.rs
use rusqlite::{params, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub provider: String,
    pub email: String,
    pub display_name: Option<String>,
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub token_expires_at: Option<i64>,
    pub imap_host: Option<String>,
    pub imap_port: Option<i32>,
    pub smtp_host: Option<String>,
    pub smtp_port: Option<i32>,
    pub username: Option<String>,
    pub sync_status: String,
    pub sync_error: Option<String>,
    pub is_active: bool,
    pub created_at: i64,
}

#[derive(Debug, Deserialize)]
pub struct CreateAccount {
    pub provider: String,
    pub email: String,
    pub display_name: Option<String>,
    pub imap_host: Option<String>,
    pub imap_port: Option<i32>,
    pub smtp_host: Option<String>,
    pub smtp_port: Option<i32>,
    pub username: Option<String>,
    pub password: Option<String>,
}

impl Account {
    pub fn from_row(row: &Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            provider: row.get("provider")?,
            email: row.get("email")?,
            display_name: row.get("display_name")?,
            access_token: row.get("access_token")?,
            refresh_token: row.get("refresh_token")?,
            token_expires_at: row.get("token_expires_at")?,
            imap_host: row.get("imap_host")?,
            imap_port: row.get("imap_port")?,
            smtp_host: row.get("smtp_host")?,
            smtp_port: row.get("smtp_port")?,
            username: row.get("username")?,
            sync_status: row.get("sync_status")?,
            sync_error: row.get("sync_error")?,
            is_active: row.get::<_, i32>("is_active")? != 0,
            created_at: row.get("created_at")?,
        })
    }

    pub fn list(conn: &rusqlite::Connection) -> rusqlite::Result<Vec<Self>> {
        let mut stmt = conn.prepare(
            "SELECT * FROM accounts WHERE is_active = 1 ORDER BY created_at"
        )?;
        let rows = stmt.query_map([], Self::from_row)?;
        rows.collect()
    }

    pub fn get_by_id(conn: &rusqlite::Connection, id: &str) -> rusqlite::Result<Self> {
        conn.query_row("SELECT * FROM accounts WHERE id = ?1", params![id], Self::from_row)
    }

    pub fn create(conn: &rusqlite::Connection, input: &CreateAccount) -> rusqlite::Result<Self> {
        let id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO accounts (id, provider, email, display_name, imap_host, imap_port, smtp_host, smtp_port, username, password_encrypted)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![id, input.provider, input.email, input.display_name, input.imap_host, input.imap_port, input.smtp_host, input.smtp_port, input.username, input.password],
        )?;
        Self::get_by_id(conn, &id)
    }

    pub fn update_oauth_tokens(
        conn: &rusqlite::Connection,
        id: &str,
        access_token: &str,
        refresh_token: &str,
        expires_at: i64,
    ) -> rusqlite::Result<()> {
        conn.execute(
            "UPDATE accounts SET access_token = ?1, refresh_token = ?2, token_expires_at = ?3, updated_at = unixepoch() WHERE id = ?4",
            params![access_token, refresh_token, expires_at, id],
        )?;
        Ok(())
    }

    pub fn update_sync_status(
        conn: &rusqlite::Connection,
        id: &str,
        status: &str,
        error: Option<&str>,
    ) -> rusqlite::Result<()> {
        conn.execute(
            "UPDATE accounts SET sync_status = ?1, sync_error = ?2, last_sync_at = unixepoch(), updated_at = unixepoch() WHERE id = ?3",
            params![status, error, id],
        )?;
        Ok(())
    }
}
```

**Step 2: Write Message model**

```rust
// src/models/message.rs
use rusqlite::{params, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub account_id: String,
    pub message_id: Option<String>,
    pub thread_id: Option<String>,
    pub folder: String,
    pub from_address: Option<String>,
    pub from_name: Option<String>,
    pub to_addresses: Option<String>,
    pub subject: Option<String>,
    pub snippet: Option<String>,
    pub date: Option<i64>,
    pub is_read: bool,
    pub is_starred: bool,
    pub has_attachments: bool,
    pub attachment_names: Option<String>,
    pub labels: Option<String>,
    // AI fields (null until V6)
    pub ai_priority_label: Option<String>,
    pub ai_category: Option<String>,
    pub ai_deadline: Option<String>,
}

/// Lightweight row for inbox list (not full body)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSummary {
    pub id: String,
    pub account_id: String,
    pub thread_id: Option<String>,
    pub folder: String,
    pub from_address: Option<String>,
    pub from_name: Option<String>,
    pub subject: Option<String>,
    pub snippet: Option<String>,
    pub date: Option<i64>,
    pub is_read: bool,
    pub is_starred: bool,
    pub has_attachments: bool,
    pub labels: Option<String>,
    pub ai_priority_label: Option<String>,
    pub ai_category: Option<String>,
}

impl MessageSummary {
    pub fn from_row(row: &Row) -> rusqlite::Result<Self> {
        Ok(Self {
            id: row.get("id")?,
            account_id: row.get("account_id")?,
            thread_id: row.get("thread_id")?,
            folder: row.get("folder")?,
            from_address: row.get("from_address")?,
            from_name: row.get("from_name")?,
            subject: row.get("subject")?,
            snippet: row.get("snippet")?,
            date: row.get("date")?,
            is_read: row.get::<_, i32>("is_read")? != 0,
            is_starred: row.get::<_, i32>("is_starred")? != 0,
            has_attachments: row.get::<_, i32>("has_attachments")? != 0,
            labels: row.get("labels")?,
            ai_priority_label: row.get("ai_priority_label")?,
            ai_category: row.get("ai_category")?,
        })
    }

    pub fn list_by_folder(
        conn: &rusqlite::Connection,
        account_id: &str,
        folder: &str,
        limit: i64,
        offset: i64,
    ) -> rusqlite::Result<Vec<Self>> {
        let mut stmt = conn.prepare(
            "SELECT id, account_id, thread_id, folder, from_address, from_name,
                    subject, snippet, date, is_read, is_starred, has_attachments,
                    labels, ai_priority_label, ai_category
             FROM messages
             WHERE account_id = ?1 AND folder = ?2 AND is_deleted = 0
             ORDER BY date DESC
             LIMIT ?3 OFFSET ?4"
        )?;
        let rows = stmt.query_map(params![account_id, folder, limit, offset], Self::from_row)?;
        rows.collect()
    }
}

/// For inserting synced messages
pub struct InsertMessage {
    pub account_id: String,
    pub message_id: Option<String>,
    pub thread_id: Option<String>,
    pub folder: String,
    pub from_address: Option<String>,
    pub from_name: Option<String>,
    pub to_addresses: Option<String>,
    pub cc_addresses: Option<String>,
    pub subject: Option<String>,
    pub snippet: Option<String>,
    pub body_text: Option<String>,
    pub body_html: Option<String>,
    pub date: Option<i64>,
    pub uid: Option<u32>,
    pub is_read: bool,
    pub has_attachments: bool,
    pub attachment_names: Option<String>,
    pub raw_headers: Option<String>,
    pub size_bytes: Option<i64>,
}

impl InsertMessage {
    pub fn insert(conn: &rusqlite::Connection, msg: &Self) -> rusqlite::Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT OR IGNORE INTO messages
             (id, account_id, message_id, thread_id, folder, from_address, from_name,
              to_addresses, cc_addresses, subject, snippet, body_text, body_html,
              date, uid, is_read, has_attachments, attachment_names, raw_headers, size_bytes)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
            params![
                id, msg.account_id, msg.message_id, msg.thread_id, msg.folder,
                msg.from_address, msg.from_name, msg.to_addresses, msg.cc_addresses,
                msg.subject, msg.snippet, msg.body_text, msg.body_html,
                msg.date, msg.uid, msg.is_read as i32, msg.has_attachments as i32,
                msg.attachment_names, msg.raw_headers, msg.size_bytes,
            ],
        )?;
        Ok(id)
    }
}

pub fn unread_count(conn: &rusqlite::Connection, account_id: &str, folder: &str) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT COUNT(*) FROM messages WHERE account_id = ?1 AND folder = ?2 AND is_read = 0 AND is_deleted = 0",
        params![account_id, folder],
        |row| row.get(0),
    )
}
```

**Step 3: Write model tests**

Add tests to `src/models/account.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    #[test]
    fn test_create_and_list_accounts() {
        let pool = db::create_test_pool();
        let conn = pool.get().unwrap();

        let input = CreateAccount {
            provider: "gmail".into(),
            email: "test@gmail.com".into(),
            display_name: Some("Test User".into()),
            imap_host: None, imap_port: None, smtp_host: None,
            smtp_port: None, username: None, password: None,
        };

        let account = Account::create(&conn, &input).unwrap();
        assert_eq!(account.email, "test@gmail.com");
        assert_eq!(account.sync_status, "pending");

        let accounts = Account::list(&conn).unwrap();
        assert_eq!(accounts.len(), 1);
    }
}
```

**Step 4: Run tests**

Run: `cargo test models`
Expected: PASS.

**Step 5: Commit**

```bash
git add src/models/
git commit -m "feat(v1): Account and Message models with DB operations"
```

---

## Task 5: Axum HTTP Server + Health + Static Files

**Files:**
- Modify: `src/main.rs`
- Create: `src/api/mod.rs`
- Create: `src/api/health.rs`

**Step 1: Write health endpoint**

```rust
// src/api/health.rs
use axum::Json;
use serde::Serialize;

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
}

pub async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}
```

```rust
// src/api/mod.rs
pub mod health;
```

**Step 2: Wire up Axum server in main.rs**

```rust
// src/main.rs
mod api;
mod config;
mod db;
mod models;

use axum::{Router, routing::get};
use config::Config;
use std::net::SocketAddr;
use tower_http::cors::{CorsLayer, Any};
use tower_http::services::{ServeDir, ServeFile};

pub struct AppState {
    pub db: db::DbPool,
    pub config: Config,
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt().with_env_filter("iris_server=debug,info").init();

    let config = Config::from_env();
    let pool = db::create_pool(&config.database_url).expect("Failed to create database pool");

    // Run migrations
    {
        let conn = pool.get().expect("Failed to get DB connection");
        db::migrations::run(&conn).expect("Failed to run migrations");
    }

    let state = std::sync::Arc::new(AppState { db: pool, config: config.clone() });

    // API routes
    let api_routes = Router::new()
        .route("/health", get(api::health::health));

    // SPA fallback: serve static files, fall back to index.html for client-side routing
    let spa = ServeDir::new("web/dist").fallback(ServeFile::new("web/dist/index.html"));

    let app = Router::new()
        .nest("/api", api_routes)
        .fallback_service(spa)
        .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any))
        .with_state(state);

    let addr = SocketAddr::from(([0, 0, 0, 0], config.port));
    tracing::info!("Iris listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
```

**Step 3: Write integration test for health**

```rust
// tests/health_test.rs
use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

// Note: integration tests will be added after the full router is extractable.
// For now, test via cargo run + curl.
```

**Step 4: Verify server starts**

Run: `cargo run &` then `curl http://localhost:3000/api/health`
Expected: `{"status":"ok","version":"0.1.0"}`

**Step 5: Commit**

```bash
git add src/
git commit -m "feat(v1): Axum server with health endpoint and SPA static serving"
```

---

## Task 6: WebSocket Server

**Files:**
- Create: `src/ws/mod.rs`
- Create: `src/ws/hub.rs`
- Modify: `src/main.rs` (add WS route)

**Step 1: Write WebSocket hub (broadcast to connected clients)**

```rust
// src/ws/hub.rs
use std::sync::Arc;
use tokio::sync::broadcast;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", content = "data")]
pub enum WsEvent {
    NewEmail { account_id: String, message_id: String },
    SyncStatus { account_id: String, status: String, progress: Option<f32> },
    SyncComplete { account_id: String },
}

#[derive(Clone)]
pub struct WsHub {
    sender: broadcast::Sender<String>,
}

impl WsHub {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(256);
        Self { sender }
    }

    pub fn broadcast(&self, event: WsEvent) {
        let json = serde_json::to_string(&event).unwrap_or_default();
        // Ignore error when no receivers
        let _ = self.sender.send(json);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.sender.subscribe()
    }
}
```

```rust
// src/ws/mod.rs
pub mod hub;

use axum::{
    extract::{State, WebSocketUpgrade, ws::{Message, WebSocket}},
    response::IntoResponse,
};
use std::sync::Arc;
use crate::AppState;

pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: Arc<AppState>) {
    let mut rx = state.ws_hub.subscribe();

    loop {
        tokio::select! {
            msg = rx.recv() => {
                match msg {
                    Ok(text) => {
                        if socket.send(Message::Text(text.into())).await.is_err() {
                            break; // Client disconnected
                        }
                    }
                    Err(_) => break,
                }
            }
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Ping(data))) => {
                        let _ = socket.send(Message::Pong(data)).await;
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {} // Ignore other messages for now
                }
            }
        }
    }
}
```

**Step 2: Add WsHub to AppState and wire route**

Update `src/main.rs` AppState:

```rust
pub struct AppState {
    pub db: db::DbPool,
    pub config: Config,
    pub ws_hub: ws::hub::WsHub,
}
```

Add `mod ws;` and add route: `.route("/ws", get(ws::ws_handler))`

Initialize: `ws_hub: ws::hub::WsHub::new()`

**Step 3: Verify compilation**

Run: `cargo build`
Expected: Compiles.

**Step 4: Commit**

```bash
git add src/ws/
git commit -m "feat(v1): WebSocket server with broadcast hub"
```

---

## Task 7: OAuth2 Authentication

**Files:**
- Create: `src/auth/mod.rs`
- Create: `src/auth/oauth.rs`
- Modify: `src/main.rs` (add auth routes)

**Step 1: Write OAuth2 provider configuration**

```rust
// src/auth/mod.rs
pub mod oauth;
```

```rust
// src/auth/oauth.rs
use axum::{
    extract::{Path, Query, State},
    response::{IntoResponse, Redirect},
    Json,
};
use oauth2::{
    AuthorizationCode, AuthUrl, ClientId, ClientSecret, CsrfToken,
    RedirectUrl, Scope, TokenUrl, TokenResponse,
    basic::BasicClient, reqwest::async_http_client,
};
use serde::Deserialize;
use std::sync::Arc;
use crate::AppState;
use crate::models::account::Account;

struct ProviderConfig {
    auth_url: &'static str,
    token_url: &'static str,
    scopes: Vec<&'static str>,
}

fn provider_config(provider: &str) -> Option<ProviderConfig> {
    match provider {
        "gmail" => Some(ProviderConfig {
            auth_url: "https://accounts.google.com/o/oauth2/v2/auth",
            token_url: "https://oauth2.googleapis.com/token",
            scopes: vec!["https://mail.google.com/", "openid", "email", "profile"],
        }),
        "outlook" => Some(ProviderConfig {
            auth_url: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
            token_url: "https://login.microsoftonline.com/common/oauth2/v2.0/token",
            scopes: vec![
                "https://outlook.office365.com/IMAP.AccessAsUser.All",
                "https://outlook.office365.com/SMTP.Send",
                "offline_access", "openid", "email", "profile",
            ],
        }),
        _ => None,
    }
}

/// GET /auth/oauth/:provider — returns redirect URL for OAuth2 flow
pub async fn start_oauth(
    Path(provider): Path<String>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let config = &state.config;
    let (client_id, client_secret) = match provider.as_str() {
        "gmail" => match (&config.gmail_client_id, &config.gmail_client_secret) {
            (Some(id), Some(secret)) => (id.clone(), secret.clone()),
            _ => return Err((axum::http::StatusCode::BAD_REQUEST, "Gmail OAuth not configured")),
        },
        "outlook" => match (&config.outlook_client_id, &config.outlook_client_secret) {
            (Some(id), Some(secret)) => (id.clone(), secret.clone()),
            _ => return Err((axum::http::StatusCode::BAD_REQUEST, "Outlook OAuth not configured")),
        },
        _ => return Err((axum::http::StatusCode::BAD_REQUEST, "Unsupported provider")),
    };

    let provider_cfg = provider_config(&provider).unwrap();
    let redirect_url = format!("{}/auth/callback", config.public_url);

    let client = BasicClient::new(ClientId::new(client_id))
        .set_client_secret(ClientSecret::new(client_secret))
        .set_auth_uri(AuthUrl::new(provider_cfg.auth_url.to_string()).unwrap())
        .set_token_uri(TokenUrl::new(provider_cfg.token_url.to_string()).unwrap())
        .set_redirect_uri(RedirectUrl::new(redirect_url).unwrap());

    let mut auth_request = client.authorize_url(CsrfToken::new_random);
    for scope in provider_cfg.scopes {
        auth_request = auth_request.add_scope(Scope::new(scope.to_string()));
    }
    // Gmail needs access_type=offline for refresh token
    if provider == "gmail" {
        auth_request = auth_request.add_extra_param("access_type", "offline");
        auth_request = auth_request.add_extra_param("prompt", "consent");
    }

    let (auth_url, csrf_token) = auth_request.url();

    // TODO: Store csrf_token in session/memory for validation in callback
    // For V1, we'll skip CSRF validation but log it
    tracing::debug!("OAuth CSRF token for {}: {}", provider, csrf_token.secret());

    Ok(Json(serde_json::json!({
        "url": auth_url.to_string(),
        "provider": provider,
    })))
}

#[derive(Deserialize)]
pub struct OAuthCallback {
    pub code: String,
    pub state: Option<String>,
}

/// GET /auth/callback — handles OAuth2 redirect
pub async fn oauth_callback(
    Query(params): Query<OAuthCallback>,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    // For V1, determine provider from state param or default to gmail
    // In production, encode the provider in the state param
    let provider = "gmail"; // TODO: extract from state param

    let config = &state.config;
    let (client_id, client_secret) = match provider {
        "gmail" => (
            config.gmail_client_id.as_ref().unwrap().clone(),
            config.gmail_client_secret.as_ref().unwrap().clone(),
        ),
        _ => return Err((axum::http::StatusCode::BAD_REQUEST, "Unknown provider")),
    };

    let provider_cfg = provider_config(provider).unwrap();
    let redirect_url = format!("{}/auth/callback", config.public_url);

    let client = BasicClient::new(ClientId::new(client_id))
        .set_client_secret(ClientSecret::new(client_secret))
        .set_auth_uri(AuthUrl::new(provider_cfg.auth_url.to_string()).unwrap())
        .set_token_uri(TokenUrl::new(provider_cfg.token_url.to_string()).unwrap())
        .set_redirect_uri(RedirectUrl::new(redirect_url).unwrap());

    let token_result = client
        .exchange_code(AuthorizationCode::new(params.code))
        .request_async(async_http_client)
        .await;

    match token_result {
        Ok(token) => {
            let access_token = token.access_token().secret().to_string();
            let refresh_token = token.refresh_token().map(|t| t.secret().to_string()).unwrap_or_default();
            let expires_at = token.expires_in()
                .map(|d| chrono::Utc::now().timestamp() + d.as_secs() as i64)
                .unwrap_or(0);

            // Fetch user email from Google userinfo
            let email = fetch_google_email(&access_token).await.unwrap_or_else(|_| "unknown@gmail.com".into());

            // Create or update account
            let conn = state.db.get().unwrap();
            let input = crate::models::account::CreateAccount {
                provider: provider.to_string(),
                email: email.clone(),
                display_name: None,
                imap_host: Some("imap.gmail.com".into()),
                imap_port: Some(993),
                smtp_host: Some("smtp.gmail.com".into()),
                smtp_port: Some(587),
                username: Some(email),
                password: None,
            };

            let account = Account::create(&conn, &input).unwrap();
            Account::update_oauth_tokens(&conn, &account.id, &access_token, &refresh_token, expires_at).unwrap();

            // Redirect to frontend with success
            Ok(Redirect::to(&format!("/setup/success?account_id={}", account.id)))
        }
        Err(e) => {
            tracing::error!("OAuth token exchange failed: {:?}", e);
            Ok(Redirect::to("/setup/error"))
        }
    }
}

async fn fetch_google_email(access_token: &str) -> Result<String, Box<dyn std::error::Error>> {
    let client = reqwest::Client::new();
    let resp: serde_json::Value = client
        .get("https://www.googleapis.com/oauth2/v2/userinfo")
        .bearer_auth(access_token)
        .send()
        .await?
        .json()
        .await?;
    Ok(resp["email"].as_str().unwrap_or("unknown").to_string())
}
```

**Step 2: Add auth routes to main.rs**

```rust
// In the api_routes Router:
.route("/auth/oauth/{provider}", get(auth::oauth::start_oauth))
.route("/auth/callback", get(auth::oauth::oauth_callback))
```

**Step 3: Verify compilation**

Run: `cargo build`
Expected: Compiles.

**Step 4: Commit**

```bash
git add src/auth/
git commit -m "feat(v1): OAuth2 authentication flow for Gmail and Outlook"
```

---

## Task 8: Account Management API

**Files:**
- Create: `src/api/accounts.rs`
- Modify: `src/api/mod.rs`
- Modify: `src/main.rs` (add routes)

**Step 1: Write account API handlers**

```rust
// src/api/accounts.rs
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use std::sync::Arc;
use crate::AppState;
use crate::models::account::{Account, CreateAccount};

pub async fn list_accounts(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<Account>>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let accounts = Account::list(&conn).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(accounts))
}

pub async fn get_account(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Account>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let account = Account::get_by_id(&conn, &id).map_err(|_| StatusCode::NOT_FOUND)?;
    Ok(Json(account))
}

/// Create account with manual IMAP config (non-OAuth path)
pub async fn create_account(
    State(state): State<Arc<AppState>>,
    Json(input): Json<CreateAccount>,
) -> Result<(StatusCode, Json<Account>), StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let account = Account::create(&conn, &input).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok((StatusCode::CREATED, Json(account)))
}

pub async fn delete_account(
    Path(id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<StatusCode, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    conn.execute("UPDATE accounts SET is_active = 0 WHERE id = ?1", rusqlite::params![id])
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}
```

**Step 2: Wire routes**

```rust
// In api_routes:
.route("/accounts", get(api::accounts::list_accounts).post(api::accounts::create_account))
.route("/accounts/{id}", get(api::accounts::get_account).delete(api::accounts::delete_account))
```

**Step 3: Verify and commit**

Run: `cargo build`

```bash
git add src/api/accounts.rs src/api/mod.rs
git commit -m "feat(v1): account CRUD API endpoints"
```

---

## Task 9: Messages API

**Files:**
- Create: `src/api/messages.rs`
- Modify: `src/api/mod.rs`

**Step 1: Write messages API handler**

```rust
// src/api/messages.rs
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use std::sync::Arc;
use crate::AppState;
use crate::models::message::{MessageSummary, unread_count};

#[derive(Deserialize)]
pub struct ListMessagesQuery {
    pub account_id: Option<String>,
    pub folder: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(serde::Serialize)]
pub struct MessagesResponse {
    pub messages: Vec<MessageSummary>,
    pub unread_count: i64,
    pub total: i64,
}

pub async fn list_messages(
    Query(params): Query<ListMessagesQuery>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<MessagesResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let folder = params.folder.as_deref().unwrap_or("INBOX");
    let limit = params.limit.unwrap_or(50);
    let offset = params.offset.unwrap_or(0);

    // If no account specified, get messages from all active accounts
    let account_id = params.account_id.clone().unwrap_or_default();

    if account_id.is_empty() {
        // All accounts - unified inbox
        let mut all_messages = Vec::new();
        let accounts = crate::models::account::Account::list(&conn)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        for acc in &accounts {
            let msgs = MessageSummary::list_by_folder(&conn, &acc.id, folder, limit, offset)
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            all_messages.extend(msgs);
        }
        // Sort by date descending
        all_messages.sort_by(|a, b| b.date.cmp(&a.date));
        all_messages.truncate(limit as usize);

        let total: i64 = conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE folder = ?1 AND is_deleted = 0",
            rusqlite::params![folder],
            |row| row.get(0),
        ).unwrap_or(0);

        let unread: i64 = conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE folder = ?1 AND is_read = 0 AND is_deleted = 0",
            rusqlite::params![folder],
            |row| row.get(0),
        ).unwrap_or(0);

        Ok(Json(MessagesResponse { messages: all_messages, unread_count: unread, total }))
    } else {
        let messages = MessageSummary::list_by_folder(&conn, &account_id, folder, limit, offset)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let unread = unread_count(&conn, &account_id, folder).unwrap_or(0);
        let total: i64 = conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE account_id = ?1 AND folder = ?2 AND is_deleted = 0",
            rusqlite::params![account_id, folder],
            |row| row.get(0),
        ).unwrap_or(0);

        Ok(Json(MessagesResponse { messages, unread_count: unread, total }))
    }
}
```

**Step 2: Wire route**

```rust
.route("/messages", get(api::messages::list_messages))
```

**Step 3: Commit**

```bash
git add src/api/messages.rs
git commit -m "feat(v1): messages list API with pagination and unified inbox"
```

---

## Task 10: IMAP Connection Manager

**Files:**
- Create: `src/imap/mod.rs`
- Create: `src/imap/connection.rs`

**Step 1: Write IMAP connection manager**

```rust
// src/imap/mod.rs
pub mod connection;
pub mod sync;
pub mod idle;
```

```rust
// src/imap/connection.rs
use async_imap::Session;
use async_native_tls::TlsStream;
use tokio::net::TcpStream;

pub type ImapSession = Session<TlsStream<TcpStream>>;

pub struct ImapCredentials {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub auth: ImapAuth,
}

pub enum ImapAuth {
    OAuth2 { access_token: String },
    Password { password: String },
}

pub async fn connect(creds: &ImapCredentials) -> Result<ImapSession, Box<dyn std::error::Error + Send + Sync>> {
    let tls = async_native_tls::TlsConnector::new();
    let tcp = TcpStream::connect(format!("{}:{}", creds.host, creds.port)).await?;
    let tls_stream = tls.connect(&creds.host, tcp).await?;

    let client = async_imap::Client::new(tls_stream);
    let mut session = match &creds.auth {
        ImapAuth::OAuth2 { access_token } => {
            // Gmail XOAUTH2 SASL mechanism
            let auth_string = format!(
                "user={}\x01auth=Bearer {}\x01\x01",
                creds.username, access_token
            );
            client.authenticate("XOAUTH2", &ImapOAuth2 { auth_string }).await
                .map_err(|e| format!("IMAP XOAUTH2 auth failed: {:?}", e.0))?
        }
        ImapAuth::Password { password } => {
            client.login(&creds.username, password).await
                .map_err(|e| format!("IMAP login failed: {:?}", e.0))?
        }
    };

    tracing::info!("IMAP connected to {} as {}", creds.host, creds.username);
    Ok(session)
}

struct ImapOAuth2 {
    auth_string: String,
}

impl async_imap::Authenticator for ImapOAuth2 {
    type Response = String;
    fn process(&mut self, _data: &[u8]) -> Self::Response {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(&self.auth_string)
    }
}
```

**Step 2: Add `base64` dependency to Cargo.toml**

```toml
base64 = "0.22"
```

**Step 3: Verify compilation**

Run: `cargo build`

**Step 4: Commit**

```bash
git add src/imap/ Cargo.toml
git commit -m "feat(v1): IMAP connection manager with OAuth2 and password auth"
```

---

## Task 11: Initial Sync Engine

**Files:**
- Create: `src/imap/sync.rs`

**Step 1: Write sync engine**

```rust
// src/imap/sync.rs
use crate::db::DbPool;
use crate::imap::connection::{ImapSession, ImapCredentials, connect};
use crate::models::message::InsertMessage;
use crate::ws::hub::{WsHub, WsEvent};
use async_imap::types::Fetch;

pub struct SyncEngine {
    pub db: DbPool,
    pub ws_hub: WsHub,
}

impl SyncEngine {
    /// Perform initial sync for an account: list folders, then fetch messages newest-first
    pub async fn initial_sync(
        &self,
        account_id: &str,
        creds: &ImapCredentials,
    ) -> Result<u32, Box<dyn std::error::Error + Send + Sync>> {
        // Update sync status
        {
            let conn = self.db.get()?;
            crate::models::account::Account::update_sync_status(&conn, account_id, "syncing", None)?;
        }
        self.ws_hub.broadcast(WsEvent::SyncStatus {
            account_id: account_id.to_string(),
            status: "syncing".to_string(),
            progress: Some(0.0),
        });

        let mut session = connect(creds).await?;

        // Fetch INBOX (primary folder for V1)
        let mailbox = session.select("INBOX").await?;
        let total = mailbox.exists;
        tracing::info!("INBOX has {} messages for account {}", total, account_id);

        if total == 0 {
            self.finish_sync(&mut session, account_id, 0).await?;
            return Ok(0);
        }

        // Fetch newest-first: UIDs from total down to max(1, total-499) for first batch
        let batch_size = 100u32;
        let start = total.saturating_sub(batch_size - 1).max(1);
        let range = format!("{}:{}", start, total);

        let fetches = session
            .fetch(&range, "(UID FLAGS ENVELOPE BODY.PEEK[TEXT] RFC822.SIZE BODY.PEEK[HEADER])")
            .await?;

        let mut count = 0u32;
        let conn = self.db.get()?;
        let fetches: Vec<_> = fetches.collect::<Vec<_>>().await;

        for fetch_result in &fetches {
            if let Ok(fetch) = fetch_result {
                if let Some(msg) = parse_fetch(fetch, account_id) {
                    match InsertMessage::insert(&conn, &msg) {
                        Ok(msg_id) => {
                            count += 1;
                            self.ws_hub.broadcast(WsEvent::NewEmail {
                                account_id: account_id.to_string(),
                                message_id: msg_id,
                            });
                        }
                        Err(e) => tracing::warn!("Failed to insert message: {}", e),
                    }
                }

                // Broadcast progress
                let progress = count as f32 / total as f32;
                self.ws_hub.broadcast(WsEvent::SyncStatus {
                    account_id: account_id.to_string(),
                    status: "syncing".to_string(),
                    progress: Some(progress.min(1.0)),
                });
            }
        }

        self.finish_sync(&mut session, account_id, count).await?;
        Ok(count)
    }

    async fn finish_sync(
        &self,
        session: &mut ImapSession,
        account_id: &str,
        count: u32,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let _ = session.logout().await;
        let conn = self.db.get()?;
        crate::models::account::Account::update_sync_status(&conn, account_id, "idle", None)?;
        self.ws_hub.broadcast(WsEvent::SyncComplete {
            account_id: account_id.to_string(),
        });
        tracing::info!("Sync complete for {}: {} messages", account_id, count);
        Ok(())
    }
}

fn parse_fetch(fetch: &Fetch, account_id: &str) -> Option<InsertMessage> {
    let envelope = fetch.envelope()?;
    let uid = fetch.uid?;

    let from_address = envelope.from.as_ref()
        .and_then(|addrs| addrs.first())
        .and_then(|addr| {
            let mailbox = addr.mailbox.as_ref().map(|m| String::from_utf8_lossy(m).to_string())?;
            let host = addr.host.as_ref().map(|h| String::from_utf8_lossy(h).to_string())?;
            Some(format!("{}@{}", mailbox, host))
        });

    let from_name = envelope.from.as_ref()
        .and_then(|addrs| addrs.first())
        .and_then(|addr| addr.name.as_ref())
        .map(|n| String::from_utf8_lossy(n).to_string());

    let subject = envelope.subject.as_ref()
        .map(|s| String::from_utf8_lossy(s).to_string());

    let message_id = envelope.message_id.as_ref()
        .map(|m| String::from_utf8_lossy(m).to_string());

    // Extract date from envelope
    let date = envelope.date.as_ref()
        .and_then(|d| {
            let date_str = String::from_utf8_lossy(d);
            chrono::DateTime::parse_from_rfc2822(&date_str).ok()
                .map(|dt| dt.timestamp())
        });

    // Body text
    let body_text = fetch.text()
        .map(|b| String::from_utf8_lossy(b).to_string());

    let snippet = body_text.as_ref()
        .map(|t| t.chars().take(200).collect::<String>());

    // Check flags
    let flags = fetch.flags();
    let is_read = flags.iter().any(|f| matches!(f, async_imap::types::Flag::Seen));

    // Thread ID from message_id (simplified — proper threading uses References/In-Reply-To)
    let thread_id = message_id.clone();

    // Raw headers for future trust badge parsing (V9)
    let raw_headers = fetch.header()
        .map(|h| String::from_utf8_lossy(h).to_string());

    Some(InsertMessage {
        account_id: account_id.to_string(),
        message_id,
        thread_id,
        folder: "INBOX".to_string(),
        from_address,
        from_name,
        to_addresses: None, // TODO: parse from envelope
        cc_addresses: None,
        subject,
        snippet,
        body_text,
        body_html: None, // TODO: parse HTML part
        date,
        uid: Some(uid),
        is_read,
        has_attachments: false, // TODO: detect from structure
        attachment_names: None,
        raw_headers,
        size_bytes: fetch.size.map(|s| s as i64),
    })
}
```

**Step 2: Verify compilation**

Run: `cargo build`

**Step 3: Commit**

```bash
git add src/imap/sync.rs
git commit -m "feat(v1): initial IMAP sync engine — fetches inbox newest-first"
```

---

## Task 12: IMAP IDLE Listener

**Files:**
- Create: `src/imap/idle.rs`

**Step 1: Write IDLE listener**

```rust
// src/imap/idle.rs
use crate::db::DbPool;
use crate::imap::connection::{ImapCredentials, connect};
use crate::imap::sync::SyncEngine;
use crate::ws::hub::WsHub;
use std::time::Duration;

/// Spawn a background task that keeps an IMAP IDLE connection open.
/// When new mail arrives, triggers a fetch and WebSocket push.
pub fn spawn_idle_listener(
    account_id: String,
    creds: ImapCredentials,
    db: DbPool,
    ws_hub: WsHub,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            match run_idle(&account_id, &creds, &db, &ws_hub).await {
                Ok(_) => tracing::info!("IDLE session ended for {}, reconnecting...", account_id),
                Err(e) => {
                    tracing::error!("IDLE error for {}: {}, retrying in 30s...", account_id, e);
                    tokio::time::sleep(Duration::from_secs(30)).await;
                }
            }
        }
    })
}

async fn run_idle(
    account_id: &str,
    creds: &ImapCredentials,
    db: &DbPool,
    ws_hub: &WsHub,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut session = connect(creds).await?;
    session.select("INBOX").await?;

    loop {
        // Enter IDLE mode
        let mut idle = session.idle();
        idle.init().await?;

        // Wait for server notification or timeout (29 min — RFC recommends < 30 min)
        let result = idle.wait_with_timeout(Duration::from_secs(29 * 60)).await?;

        // Break out of IDLE — session is now back to normal
        let session_back = idle.done().await?;
        session = session_back;

        // If we got a notification (not just timeout), fetch new messages
        // For V1: do a lightweight re-sync by checking for new UIDs
        tracing::debug!("IDLE notification for {}: new mail likely", account_id);

        let sync_engine = SyncEngine {
            db: db.clone(),
            ws_hub: ws_hub.clone(),
        };
        // Re-sync will pick up new messages
        if let Err(e) = sync_engine.initial_sync(account_id, creds).await {
            tracing::error!("Re-sync after IDLE failed for {}: {}", account_id, e);
        }
    }
}
```

**Step 2: Wire sync trigger after account creation**

Add to `src/auth/oauth.rs` after account creation — spawn sync + IDLE:

```rust
// After account is created in oauth_callback:
let sync_db = state.db.clone();
let sync_ws = state.ws_hub.clone();
let sync_account_id = account.id.clone();
let sync_creds = ImapCredentials {
    host: "imap.gmail.com".to_string(),
    port: 993,
    username: account.email.clone(),
    auth: ImapAuth::OAuth2 { access_token: access_token.clone() },
};

tokio::spawn(async move {
    let engine = SyncEngine { db: sync_db.clone(), ws_hub: sync_ws.clone() };
    if let Err(e) = engine.initial_sync(&sync_account_id, &sync_creds).await {
        tracing::error!("Initial sync failed: {}", e);
    }
    // Start IDLE listener after initial sync
    spawn_idle_listener(sync_account_id, sync_creds, sync_db, sync_ws);
});
```

**Step 3: Commit**

```bash
git add src/imap/idle.rs
git commit -m "feat(v1): IMAP IDLE listener for real-time new mail detection"
```

---

## Task 13: Config API (Theme)

**Files:**
- Create: `src/api/config.rs`

**Step 1: Write config endpoints**

```rust
// src/api/config.rs
use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use crate::AppState;

#[derive(Serialize)]
pub struct ConfigResponse {
    pub theme: String,
}

pub async fn get_config(
    State(state): State<Arc<AppState>>,
) -> Result<Json<ConfigResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let theme: String = conn
        .query_row("SELECT value FROM config WHERE key = 'theme'", [], |row| row.get(0))
        .unwrap_or_else(|_| "system".to_string());
    Ok(Json(ConfigResponse { theme }))
}

#[derive(Deserialize)]
pub struct UpdateTheme {
    pub theme: String,
}

pub async fn set_theme(
    State(state): State<Arc<AppState>>,
    Json(input): Json<UpdateTheme>,
) -> Result<StatusCode, StatusCode> {
    if !["light", "dark", "system"].contains(&input.theme.as_str()) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    conn.execute(
        "INSERT OR REPLACE INTO config (key, value, updated_at) VALUES ('theme', ?1, unixepoch())",
        rusqlite::params![input.theme],
    ).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::OK)
}
```

**Step 2: Wire route**

```rust
.route("/config", get(api::config::get_config))
.route("/config/theme", axum::routing::put(api::config::set_theme))
```

**Step 3: Commit**

```bash
git add src/api/config.rs
git commit -m "feat(v1): config API with theme get/set"
```

---

## Task 14: Frontend SPA Scaffolding

**Files:**
- Create: `web/` directory with Svelte 5 + Vite + Tailwind

**Step 1: Initialize Svelte project**

```bash
cd web
npm create vite@latest . -- --template svelte-ts
npm install
npm install -D tailwindcss @tailwindcss/vite
npm install svelte-spa-router
```

**Step 2: Configure Vite for dev proxy**

```typescript
// web/vite.config.ts
import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import tailwindcss from '@tailwindcss/vite';

export default defineConfig({
  plugins: [svelte(), tailwindcss()],
  server: {
    port: 5173,
    proxy: {
      '/api': 'http://localhost:3000',
      '/auth': 'http://localhost:3000',
      '/ws': {
        target: 'ws://localhost:3000',
        ws: true,
      },
    },
  },
  build: {
    outDir: 'dist',
  },
});
```

**Step 3: Configure Tailwind**

```css
/* web/src/app.css */
@import "tailwindcss";
```

**Step 4: Write API client**

```typescript
// web/src/lib/api.ts
const BASE = '';

async function request<T>(path: string, options?: RequestInit): Promise<T> {
  const res = await fetch(`${BASE}${path}`, {
    headers: { 'Content-Type': 'application/json' },
    ...options,
  });
  if (!res.ok) throw new Error(`API error: ${res.status}`);
  return res.json();
}

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

**Step 5: Commit**

```bash
git add web/
git commit -m "feat(v1): Svelte 5 SPA scaffolding with Vite, Tailwind, and API client"
```

---

## Task 15: App Shell + Routing

**Files:**
- Create: `web/src/App.svelte`
- Create: `web/src/components/AppShell.svelte`
- Create: `web/src/components/Sidebar.svelte`
- Create: `web/src/components/Header.svelte`
- Create: `web/src/pages/Inbox.svelte` (placeholder)
- Create: `web/src/pages/AccountSetup.svelte` (placeholder)
- Create: `web/src/pages/Settings.svelte` (placeholder)

**Step 1: Write App with routing**

```svelte
<!-- web/src/App.svelte -->
<script lang="ts">
  import Router, { push } from 'svelte-spa-router';
  import AppShell from './components/AppShell.svelte';
  import Inbox from './pages/Inbox.svelte';
  import AccountSetup from './pages/AccountSetup.svelte';
  import Settings from './pages/Settings.svelte';

  const routes = {
    '/': Inbox,
    '/setup': AccountSetup,
    '/setup/success': AccountSetup,
    '/settings': Settings,
  };
</script>

<AppShell>
  <Router {routes} />
</AppShell>
```

**Step 2: Write AppShell layout**

```svelte
<!-- web/src/components/AppShell.svelte -->
<script lang="ts">
  import Sidebar from './Sidebar.svelte';
  import Header from './Header.svelte';
  import type { Snippet } from 'svelte';

  let { children }: { children: Snippet } = $props();
</script>

<div class="flex h-screen bg-white dark:bg-gray-900 text-gray-900 dark:text-gray-100">
  <Sidebar />
  <div class="flex flex-col flex-1 overflow-hidden">
    <Header />
    <main class="flex-1 overflow-auto">
      {@render children()}
    </main>
  </div>
</div>
```

**Step 3: Write Sidebar**

```svelte
<!-- web/src/components/Sidebar.svelte -->
<script lang="ts">
  import { push, location } from 'svelte-spa-router';

  const navItems = [
    { path: '/', label: 'Inbox', icon: '📥' },
    { path: '/setup', label: 'Add Account', icon: '➕' },
    { path: '/settings', label: 'Settings', icon: '⚙️' },
  ];
</script>

<aside class="w-56 border-r border-gray-200 dark:border-gray-700 flex flex-col">
  <div class="p-4 border-b border-gray-200 dark:border-gray-700">
    <h1 class="text-xl font-bold">Iris</h1>
  </div>
  <nav class="flex-1 p-2 space-y-1">
    {#each navItems as item}
      <button
        class="w-full text-left px-3 py-2 rounded-lg text-sm transition-colors
               {$location === item.path ? 'bg-blue-50 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300 font-medium' : 'hover:bg-gray-100 dark:hover:bg-gray-800'}"
        onclick={() => push(item.path)}
      >
        <span class="mr-2">{item.icon}</span>
        {item.label}
      </button>
    {/each}
  </nav>
</aside>
```

**Step 4: Write Header**

```svelte
<!-- web/src/components/Header.svelte -->
<script lang="ts">
  // Placeholder — search bar wires to P5 in V5, chat toggle to P4 in V8
</script>

<header class="h-14 border-b border-gray-200 dark:border-gray-700 flex items-center px-4 gap-4">
  <div class="flex-1">
    <input
      type="text"
      placeholder="Search emails... (coming in V5)"
      disabled
      class="w-full max-w-md px-3 py-1.5 rounded-lg bg-gray-100 dark:bg-gray-800 text-sm
             placeholder-gray-400 cursor-not-allowed opacity-50"
    />
  </div>
  <div class="text-sm text-gray-500">
    <!-- Account switcher placeholder (V4) -->
  </div>
</header>
```

**Step 5: Write page placeholders**

```svelte
<!-- web/src/pages/Inbox.svelte -->
<script lang="ts">
  // Full implementation in Task 17
</script>

<div class="p-4">
  <h2 class="text-lg font-semibold mb-4">Inbox</h2>
  <p class="text-gray-500">Loading...</p>
</div>
```

```svelte
<!-- web/src/pages/Settings.svelte -->
<script lang="ts">
  // Theme toggle implementation in Task 18
</script>

<div class="p-4">
  <h2 class="text-lg font-semibold mb-4">Settings</h2>
</div>
```

**Step 6: Verify dev server**

Run: `cd web && npm run dev`
Expected: Opens at localhost:5173 with sidebar layout.

**Step 7: Commit**

```bash
git add web/src/
git commit -m "feat(v1): app shell with sidebar navigation and routing"
```

---

## Task 16: WebSocket Client

**Files:**
- Create: `web/src/lib/ws.ts`

**Step 1: Write reactive WebSocket client**

```typescript
// web/src/lib/ws.ts

type WsEventHandler = (event: WsEvent) => void;

interface WsEvent {
  type: 'NewEmail' | 'SyncStatus' | 'SyncComplete';
  data: any;
}

class WebSocketClient {
  private ws: WebSocket | null = null;
  private handlers: Map<string, Set<WsEventHandler>> = new Map();
  private reconnectTimer: number | null = null;
  private url: string;

  constructor() {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    this.url = `${protocol}//${window.location.host}/ws`;
  }

  connect() {
    if (this.ws?.readyState === WebSocket.OPEN) return;

    this.ws = new WebSocket(this.url);

    this.ws.onopen = () => {
      console.log('[WS] Connected');
      if (this.reconnectTimer) {
        clearTimeout(this.reconnectTimer);
        this.reconnectTimer = null;
      }
    };

    this.ws.onmessage = (event) => {
      try {
        const parsed: WsEvent = JSON.parse(event.data);
        const typeHandlers = this.handlers.get(parsed.type);
        if (typeHandlers) {
          typeHandlers.forEach((handler) => handler(parsed));
        }
        // Also fire wildcard handlers
        const allHandlers = this.handlers.get('*');
        if (allHandlers) {
          allHandlers.forEach((handler) => handler(parsed));
        }
      } catch (e) {
        console.warn('[WS] Failed to parse message:', event.data);
      }
    };

    this.ws.onclose = () => {
      console.log('[WS] Disconnected, reconnecting in 3s...');
      this.reconnectTimer = window.setTimeout(() => this.connect(), 3000);
    };

    this.ws.onerror = () => {
      this.ws?.close();
    };
  }

  on(type: string, handler: WsEventHandler) {
    if (!this.handlers.has(type)) {
      this.handlers.set(type, new Set());
    }
    this.handlers.get(type)!.add(handler);
    return () => this.handlers.get(type)?.delete(handler);
  }

  disconnect() {
    if (this.reconnectTimer) clearTimeout(this.reconnectTimer);
    this.ws?.close();
  }
}

export const wsClient = new WebSocketClient();
```

**Step 2: Commit**

```bash
git add web/src/lib/ws.ts
git commit -m "feat(v1): WebSocket client with auto-reconnect and event dispatch"
```

---

## Task 17: Account Setup Page (P7)

**Files:**
- Rewrite: `web/src/pages/AccountSetup.svelte`

**Step 1: Implement Account Setup with provider selection + OAuth**

```svelte
<!-- web/src/pages/AccountSetup.svelte -->
<script lang="ts">
  import { api } from '../lib/api';
  import { push, querystring } from 'svelte-spa-router';

  type Provider = 'gmail' | 'outlook' | 'yahoo' | 'fastmail' | 'imap';

  let step = $state<'select' | 'oauth' | 'manual' | 'syncing' | 'success' | 'error'>('select');
  let selectedProvider = $state<Provider | null>(null);
  let error = $state('');

  // Manual IMAP config
  let manualConfig = $state({
    email: '', imapHost: '', imapPort: 993,
    smtpHost: '', smtpPort: 587,
    username: '', password: '',
  });

  // Check if redirected from OAuth success
  const qs = $derived(new URLSearchParams($querystring || ''));
  $effect(() => {
    if (qs.get('account_id')) {
      step = 'success';
    }
  });

  const providers = [
    { id: 'gmail' as Provider, name: 'Gmail', oauth: true },
    { id: 'outlook' as Provider, name: 'Outlook', oauth: true },
    { id: 'yahoo' as Provider, name: 'Yahoo', oauth: false },
    { id: 'fastmail' as Provider, name: 'Fastmail', oauth: false },
    { id: 'imap' as Provider, name: 'Other IMAP', oauth: false },
  ];

  async function selectProvider(provider: Provider) {
    selectedProvider = provider;
    const p = providers.find((p) => p.id === provider);
    if (p?.oauth) {
      step = 'oauth';
    } else {
      step = 'manual';
      // Pre-fill known host configs
      if (provider === 'yahoo') {
        manualConfig.imapHost = 'imap.mail.yahoo.com';
        manualConfig.smtpHost = 'smtp.mail.yahoo.com';
      } else if (provider === 'fastmail') {
        manualConfig.imapHost = 'imap.fastmail.com';
        manualConfig.smtpHost = 'smtp.fastmail.com';
      }
    }
  }

  async function startOAuth() {
    try {
      const { url } = await api.auth.startOAuth(selectedProvider!);
      window.location.href = url;
    } catch (e: any) {
      error = e.message;
      step = 'error';
    }
  }

  async function submitManualConfig() {
    try {
      step = 'syncing';
      await api.accounts.create({
        provider: selectedProvider,
        email: manualConfig.email,
        imap_host: manualConfig.imapHost,
        imap_port: manualConfig.imapPort,
        smtp_host: manualConfig.smtpHost,
        smtp_port: manualConfig.smtpPort,
        username: manualConfig.username,
        password: manualConfig.password,
      });
      step = 'success';
    } catch (e: any) {
      error = e.message;
      step = 'error';
    }
  }
</script>

<div class="max-w-lg mx-auto p-8">
  <h2 class="text-2xl font-bold mb-6">Add Email Account</h2>

  {#if step === 'select'}
    <div class="space-y-3">
      {#each providers as provider}
        <button
          class="w-full p-4 text-left border rounded-lg hover:border-blue-500 hover:bg-blue-50
                 dark:hover:bg-blue-900/20 transition-colors"
          onclick={() => selectProvider(provider.id)}
        >
          <span class="font-medium">{provider.name}</span>
          {#if provider.oauth}
            <span class="text-xs text-gray-500 ml-2">OAuth2</span>
          {/if}
        </button>
      {/each}
    </div>

  {:else if step === 'oauth'}
    <div class="text-center space-y-4">
      <p>Connect your {selectedProvider} account securely via OAuth2.</p>
      <button
        class="px-6 py-3 bg-blue-600 text-white rounded-lg hover:bg-blue-700 font-medium"
        onclick={startOAuth}
      >
        Sign in with {selectedProvider}
      </button>
      <button class="block mx-auto text-sm text-gray-500 hover:underline" onclick={() => step = 'select'}>
        Back
      </button>
    </div>

  {:else if step === 'manual'}
    <form class="space-y-4" onsubmit|preventDefault={submitManualConfig}>
      <div>
        <label class="block text-sm font-medium mb-1">Email</label>
        <input type="email" bind:value={manualConfig.email} required
               class="w-full px-3 py-2 border rounded-lg dark:bg-gray-800 dark:border-gray-600" />
      </div>
      <div class="grid grid-cols-2 gap-4">
        <div>
          <label class="block text-sm font-medium mb-1">IMAP Host</label>
          <input type="text" bind:value={manualConfig.imapHost} required
                 class="w-full px-3 py-2 border rounded-lg dark:bg-gray-800 dark:border-gray-600" />
        </div>
        <div>
          <label class="block text-sm font-medium mb-1">IMAP Port</label>
          <input type="number" bind:value={manualConfig.imapPort}
                 class="w-full px-3 py-2 border rounded-lg dark:bg-gray-800 dark:border-gray-600" />
        </div>
      </div>
      <div class="grid grid-cols-2 gap-4">
        <div>
          <label class="block text-sm font-medium mb-1">SMTP Host</label>
          <input type="text" bind:value={manualConfig.smtpHost} required
                 class="w-full px-3 py-2 border rounded-lg dark:bg-gray-800 dark:border-gray-600" />
        </div>
        <div>
          <label class="block text-sm font-medium mb-1">SMTP Port</label>
          <input type="number" bind:value={manualConfig.smtpPort}
                 class="w-full px-3 py-2 border rounded-lg dark:bg-gray-800 dark:border-gray-600" />
        </div>
      </div>
      <div>
        <label class="block text-sm font-medium mb-1">Username</label>
        <input type="text" bind:value={manualConfig.username} required
               class="w-full px-3 py-2 border rounded-lg dark:bg-gray-800 dark:border-gray-600" />
      </div>
      <div>
        <label class="block text-sm font-medium mb-1">Password / App Password</label>
        <input type="password" bind:value={manualConfig.password} required
               class="w-full px-3 py-2 border rounded-lg dark:bg-gray-800 dark:border-gray-600" />
      </div>
      <div class="flex gap-3">
        <button type="submit" class="px-6 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700">
          Connect
        </button>
        <button type="button" class="px-6 py-2 border rounded-lg hover:bg-gray-50 dark:hover:bg-gray-800"
                onclick={() => step = 'select'}>
          Back
        </button>
      </div>
    </form>

  {:else if step === 'syncing'}
    <div class="text-center space-y-4">
      <div class="animate-spin h-8 w-8 border-4 border-blue-500 border-t-transparent rounded-full mx-auto"></div>
      <p>Syncing your emails...</p>
    </div>

  {:else if step === 'success'}
    <div class="text-center space-y-4">
      <div class="text-4xl">✓</div>
      <p class="text-lg font-medium">Account connected!</p>
      <button
        class="px-6 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-700"
        onclick={() => push('/')}
      >
        Go to Inbox
      </button>
    </div>

  {:else if step === 'error'}
    <div class="text-center space-y-4">
      <p class="text-red-600">{error || 'Something went wrong'}</p>
      <button class="text-sm text-blue-600 hover:underline" onclick={() => step = 'select'}>
        Try again
      </button>
    </div>
  {/if}
</div>
```

**Step 2: Commit**

```bash
git add web/src/pages/AccountSetup.svelte
git commit -m "feat(v1): Account Setup page with OAuth + manual IMAP config"
```

---

## Task 18: Inbox Page (P1)

**Files:**
- Rewrite: `web/src/pages/Inbox.svelte`
- Create: `web/src/components/inbox/MessageList.svelte`
- Create: `web/src/components/inbox/MessageRow.svelte`
- Create: `web/src/components/inbox/SyncStatus.svelte`
- Create: `web/src/lib/stores/messages.svelte.ts`

**Step 1: Write messages store**

```typescript
// web/src/lib/stores/messages.svelte.ts
import { api } from '../api';
import { wsClient } from '../ws';

interface MessageSummary {
  id: string;
  account_id: string;
  thread_id: string | null;
  folder: string;
  from_address: string | null;
  from_name: string | null;
  subject: string | null;
  snippet: string | null;
  date: number | null;
  is_read: boolean;
  is_starred: boolean;
  has_attachments: boolean;
  labels: string | null;
  ai_priority_label: string | null;
  ai_category: string | null;
}

export function createMessagesStore() {
  let messages = $state<MessageSummary[]>([]);
  let unreadCount = $state(0);
  let total = $state(0);
  let loading = $state(false);

  async function load(accountId?: string, folder = 'INBOX') {
    loading = true;
    try {
      const res = await api.messages.list({ account_id: accountId, folder, limit: 50 });
      messages = res.messages;
      unreadCount = res.unread_count;
      total = res.total;
    } finally {
      loading = false;
    }
  }

  // Listen for new emails via WebSocket
  function startListening() {
    wsClient.connect();
    return wsClient.on('NewEmail', () => {
      // Refresh the message list when new email arrives
      load();
    });
  }

  return {
    get messages() { return messages; },
    get unreadCount() { return unreadCount; },
    get total() { return total; },
    get loading() { return loading; },
    load,
    startListening,
  };
}
```

**Step 2: Write MessageRow component**

```svelte
<!-- web/src/components/inbox/MessageRow.svelte -->
<script lang="ts">
  let { message, onclick } = $props<{
    message: any;
    onclick: () => void;
  }>();

  function formatDate(timestamp: number | null): string {
    if (!timestamp) return '';
    const date = new Date(timestamp * 1000);
    const now = new Date();
    const isToday = date.toDateString() === now.toDateString();
    if (isToday) return date.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    return date.toLocaleDateString([], { month: 'short', day: 'numeric' });
  }

  const senderDisplay = $derived(message.from_name || message.from_address || 'Unknown');
</script>

<button
  class="w-full text-left px-4 py-3 border-b border-gray-100 dark:border-gray-800
         hover:bg-gray-50 dark:hover:bg-gray-800/50 transition-colors
         {message.is_read ? 'opacity-75' : 'font-semibold'}"
  {onclick}
>
  <div class="flex items-center gap-3">
    <!-- Unread indicator -->
    {#if !message.is_read}
      <div class="w-2 h-2 rounded-full bg-blue-500 shrink-0"></div>
    {:else}
      <div class="w-2 shrink-0"></div>
    {/if}

    <!-- Sender -->
    <div class="w-48 truncate text-sm">{senderDisplay}</div>

    <!-- Subject + Snippet -->
    <div class="flex-1 truncate text-sm">
      <span>{message.subject || '(no subject)'}</span>
      {#if message.snippet}
        <span class="text-gray-400 ml-2">— {message.snippet}</span>
      {/if}
    </div>

    <!-- Attachment indicator -->
    {#if message.has_attachments}
      <span class="text-gray-400 text-xs shrink-0">📎</span>
    {/if}

    <!-- Date -->
    <div class="text-xs text-gray-500 shrink-0 w-16 text-right">
      {formatDate(message.date)}
    </div>
  </div>
</button>
```

**Step 3: Write MessageList component**

```svelte
<!-- web/src/components/inbox/MessageList.svelte -->
<script lang="ts">
  import MessageRow from './MessageRow.svelte';

  let { messages, onSelect } = $props<{
    messages: any[];
    onSelect: (id: string) => void;
  }>();
</script>

<div class="divide-y divide-gray-100 dark:divide-gray-800">
  {#each messages as message (message.id)}
    <MessageRow {message} onclick={() => onSelect(message.id)} />
  {:else}
    <div class="p-8 text-center text-gray-500">
      No messages yet. Add an account to get started.
    </div>
  {/each}
</div>
```

**Step 4: Write SyncStatus component**

```svelte
<!-- web/src/components/inbox/SyncStatus.svelte -->
<script lang="ts">
  import { wsClient } from '../../lib/ws';

  let status = $state<string>('');
  let progress = $state<number | null>(null);

  $effect(() => {
    const unsub1 = wsClient.on('SyncStatus', (event) => {
      status = event.data.status;
      progress = event.data.progress;
    });
    const unsub2 = wsClient.on('SyncComplete', () => {
      status = '';
      progress = null;
    });
    return () => { unsub1(); unsub2(); };
  });
</script>

{#if status === 'syncing'}
  <div class="px-4 py-2 bg-blue-50 dark:bg-blue-900/20 text-sm text-blue-700 dark:text-blue-300 flex items-center gap-2">
    <div class="animate-spin h-4 w-4 border-2 border-blue-500 border-t-transparent rounded-full"></div>
    Syncing...
    {#if progress !== null}
      <span class="text-xs">({Math.round(progress * 100)}%)</span>
    {/if}
  </div>
{/if}
```

**Step 5: Wire Inbox page**

```svelte
<!-- web/src/pages/Inbox.svelte -->
<script lang="ts">
  import { onMount } from 'svelte';
  import MessageList from '../components/inbox/MessageList.svelte';
  import SyncStatus from '../components/inbox/SyncStatus.svelte';
  import { createMessagesStore } from '../lib/stores/messages.svelte';

  const store = createMessagesStore();

  onMount(() => {
    store.load();
    const unsub = store.startListening();
    return unsub;
  });

  function handleSelect(id: string) {
    // V2: navigate to thread view
    console.log('Selected message:', id);
  }
</script>

<div>
  <SyncStatus />

  <div class="flex items-center justify-between px-4 py-3 border-b border-gray-200 dark:border-gray-700">
    <div class="flex items-center gap-2">
      <h2 class="text-lg font-semibold">Inbox</h2>
      {#if store.unreadCount > 0}
        <span class="px-2 py-0.5 text-xs font-medium bg-blue-100 text-blue-700 dark:bg-blue-900/40 dark:text-blue-300 rounded-full">
          {store.unreadCount}
        </span>
      {/if}
    </div>
    <span class="text-sm text-gray-500">{store.total} messages</span>
  </div>

  {#if store.loading}
    <div class="p-8 text-center text-gray-500">Loading...</div>
  {:else}
    <MessageList messages={store.messages} onSelect={handleSelect} />
  {/if}
</div>
```

**Step 6: Commit**

```bash
git add web/src/
git commit -m "feat(v1): Inbox page with message list, unread badges, and sync status"
```

---

## Task 19: Theme Toggle (P8 minimal)

**Files:**
- Rewrite: `web/src/pages/Settings.svelte`
- Create: `web/src/lib/stores/theme.svelte.ts`

**Step 1: Write theme store**

```typescript
// web/src/lib/stores/theme.svelte.ts
import { api } from '../api';

export function createThemeStore() {
  let theme = $state<'light' | 'dark' | 'system'>('system');

  function applyTheme(t: string) {
    const isDark = t === 'dark' || (t === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches);
    document.documentElement.classList.toggle('dark', isDark);
  }

  async function load() {
    try {
      const config = await api.config.get();
      theme = config.theme as any;
      applyTheme(theme);
    } catch {
      applyTheme('system');
    }
  }

  async function set(newTheme: 'light' | 'dark' | 'system') {
    theme = newTheme;
    applyTheme(theme);
    await api.config.setTheme(newTheme);
  }

  return {
    get theme() { return theme; },
    load,
    set,
  };
}
```

**Step 2: Write Settings page with theme toggle**

```svelte
<!-- web/src/pages/Settings.svelte -->
<script lang="ts">
  import { onMount } from 'svelte';
  import { createThemeStore } from '../lib/stores/theme.svelte';

  const themeStore = createThemeStore();

  onMount(() => { themeStore.load(); });

  const themes = [
    { value: 'light' as const, label: 'Light' },
    { value: 'dark' as const, label: 'Dark' },
    { value: 'system' as const, label: 'System' },
  ];
</script>

<div class="max-w-lg mx-auto p-8">
  <h2 class="text-2xl font-bold mb-6">Settings</h2>

  <section class="mb-8">
    <h3 class="text-sm font-medium text-gray-500 uppercase tracking-wider mb-3">Appearance</h3>
    <div class="flex gap-2">
      {#each themes as t}
        <button
          class="px-4 py-2 rounded-lg border text-sm transition-colors
                 {themeStore.theme === t.value
                   ? 'border-blue-500 bg-blue-50 dark:bg-blue-900/30 text-blue-700 dark:text-blue-300'
                   : 'border-gray-200 dark:border-gray-700 hover:bg-gray-50 dark:hover:bg-gray-800'}"
          onclick={() => themeStore.set(t.value)}
        >
          {t.label}
        </button>
      {/each}
    </div>
  </section>

  <!-- Future settings sections: AI connectors (V6), categories (V6), shortcuts (V9), API keys (V9) -->
</div>
```

**Step 3: Add Tailwind dark mode config**

Tailwind CSS 4 supports dark mode via `@media (prefers-color-scheme: dark)` by default. For class-based dark mode (needed for manual toggle), add to `web/src/app.css`:

```css
@import "tailwindcss";

@custom-variant dark (&:where(.dark, .dark *));
```

**Step 4: Initialize theme on app load**

In `web/src/main.ts`, add theme initialization:

```typescript
// web/src/main.ts
import './app.css';
import App from './App.svelte';
import { mount } from 'svelte';

// Apply saved theme before first paint
const savedTheme = localStorage.getItem('iris-theme') || 'system';
const isDark = savedTheme === 'dark' || (savedTheme === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches);
document.documentElement.classList.toggle('dark', isDark);

const app = mount(App, { target: document.getElementById('app')! });

export default app;
```

**Step 5: Commit**

```bash
git add web/src/
git commit -m "feat(v1): theme toggle (light/dark/system) in settings"
```

---

## Task 20: Integration Wiring + Final Cleanup

**Files:**
- Modify: `src/main.rs` (final wiring of all routes)
- Modify: `src/api/mod.rs` (export all modules)

**Step 1: Ensure all modules are wired in main.rs**

Verify `src/main.rs` has:
- All `mod` declarations: `api`, `auth`, `config`, `db`, `imap`, `models`, `ws`
- All routes registered in the Router
- WsHub in AppState
- Database pool + migrations in startup

**Step 2: Build the full stack**

```bash
cd web && npm run build && cd ..
cargo build
```

Expected: Both compile successfully.

**Step 3: Manual smoke test**

```bash
# Terminal 1: Start the server
cargo run

# Terminal 2: Test endpoints
curl http://localhost:3000/api/health
# → {"status":"ok","version":"0.1.0"}

curl http://localhost:3000/api/accounts
# → []

curl http://localhost:3000/api/messages
# → {"messages":[],"unread_count":0,"total":0}

curl http://localhost:3000/api/config
# → {"theme":"system"}

# Test static file serving (SPA)
curl -s http://localhost:3000/ | head -5
# → Should return index.html
```

**Step 4: Commit**

```bash
git add -A
git commit -m "feat(v1): complete V1 Foundation — server, database, IMAP sync, SPA with inbox"
```

---

## Summary

| Task | Component | Key Files |
|------|-----------|-----------|
| 1 | Rust scaffolding | `Cargo.toml`, `src/main.rs`, `src/config.rs` |
| 2 | Docker Compose | `Dockerfile`, `docker-compose.yml` |
| 3 | SQLite + schema | `src/db/`, `migrations/001_initial.sql` |
| 4 | Models | `src/models/account.rs`, `src/models/message.rs` |
| 5 | HTTP server + health | `src/api/health.rs`, `src/main.rs` |
| 6 | WebSocket hub | `src/ws/hub.rs`, `src/ws/mod.rs` |
| 7 | OAuth2 auth | `src/auth/oauth.rs` |
| 8 | Accounts API | `src/api/accounts.rs` |
| 9 | Messages API | `src/api/messages.rs` |
| 10 | IMAP connection | `src/imap/connection.rs` |
| 11 | Initial sync | `src/imap/sync.rs` |
| 12 | IMAP IDLE | `src/imap/idle.rs` |
| 13 | Config API | `src/api/config.rs` |
| 14 | SPA scaffolding | `web/` (Svelte 5 + Vite + Tailwind) |
| 15 | App shell + routing | `AppShell.svelte`, `Sidebar.svelte`, `Header.svelte` |
| 16 | WebSocket client | `web/src/lib/ws.ts` |
| 17 | Account Setup (P7) | `web/src/pages/AccountSetup.svelte` |
| 18 | Inbox (P1) | `Inbox.svelte`, `MessageRow.svelte`, `SyncStatus.svelte` |
| 19 | Theme toggle (P8) | `Settings.svelte`, `theme.svelte.ts` |
| 20 | Integration wiring | Final `main.rs` wiring + smoke test |

**Affordances covered:** U2, U3, U4 (stub), U11, U12, U48-U52, U57, N20, N21, N26, N27, N40, N44, N45, N50, N51, N53, S4, S5, S11, S14
