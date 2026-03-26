# Iris

> Greek goddess of the rainbow, messenger of the gods. She bridges heaven and earth carrying messages.

**Iris** is a local-first, AI-native email client. Connect your own accounts, run your own AI, keep your data on your machine.

## Features

**Email**
- Gmail and Outlook via OAuth2, any IMAP provider with app passwords
- Full IMAP sync with IDLE push (real-time new mail)
- Threaded conversations with MIME parsing (HTML, plain text, attachments)
- Compose, reply, reply-all, forward with auto-save drafts
- Batch actions: archive, read/unread, category assignment
- Full-text search with FTS5 snippets and filter chips

**AI**
- Multi-provider: Anthropic, OpenAI, Ollama (local) — hot-swap via Settings UI
- Automatic email classification: intent, priority, category
- Entity extraction: people, dates, amounts, topics, deadlines
- Thread summarization (lazy, cached)
- AI writing assist: rewrite, formal, casual, shorter, longer
- Natural language chat with email context (RAG via Memories)
- Self-improving: user corrections feed back into future classifications

**Agent Platform**
- Unified REST API: 200+ endpoints accessible with scoped API keys
- If the UI can do it, agents can do it — same routes, same data
- 4 permission levels: read_only, draft_only, send_with_approval, autonomous
- Reply/forward endpoints with server-side threading header resolution
- MCP server with 26 tools and per-tool permission checks
- Semantic search with temporal filters (since/until) and graph-aware ranking
- Audit logging for all agent actions
- Per-key rate limiting (500 burst / 5 req/sec per key)

**Privacy & Security**
- Self-hosted: your data never leaves your machine
- No telemetry, no cloud dependencies
- Optional password gate for remote/self-hosted access
- XChaCha20-Poly1305 encryption at rest for stored credentials
- Security headers: X-Content-Type-Options, X-Frame-Options, Referrer-Policy
- Non-root Docker container (UID 1001)
- CSRF protection with same-origin validation

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) 1.85+
- [Node.js](https://nodejs.org/) 20+
- [Ollama](https://ollama.ai/) (optional, for AI features)

### Local Development

```bash
# Clone and configure
cp .env.example .env
# Edit .env with your OAuth credentials (see OAuth Setup below)

# Build and run backend
cargo build
cargo run

# In another terminal, build and run frontend
cd web
npm install
npm run dev
```

Open `http://localhost:5173` in your browser. The app will bootstrap a session cookie automatically. If `IRIS_AUTH_PASSWORD_HASH` is set, Iris will show a login screen before loading the mailbox UI.

### Docker

```bash
cp .env.example .env
# Edit .env with your OAuth credentials

docker compose up --build
```

Open `http://localhost:3000`. Ollama runs as a sidecar container. If `IRIS_AUTH_PASSWORD_HASH` is set, the web UI will require that password before it issues a session cookie.

For at-rest encryption of stored account credentials and provider API keys, set `IRIS_SECRETS_KEY` to a 32-byte base64 or 64-character hex key before first run. Existing plaintext secrets will be re-encrypted on startup once the key is configured.

For deployment hardening guidance, see [DEPLOYMENT_SECURITY.md](DEPLOYMENT_SECURITY.md).

### OAuth Setup

**Gmail:**
1. Create a project in [Google Cloud Console](https://console.cloud.google.com/)
2. Enable the Gmail API
3. Create OAuth2 credentials (Web application type)
4. Add `http://localhost:3000/auth/callback` as an authorized redirect URI
5. Set `GMAIL_CLIENT_ID` and `GMAIL_CLIENT_SECRET` in `.env`

**Outlook:**
1. Register an app in [Azure Portal](https://portal.azure.com/#blade/Microsoft_AAD_RegisteredApps)
2. Add `http://localhost:3000/auth/callback` as a redirect URI
3. Set `OUTLOOK_CLIENT_ID` and `OUTLOOK_CLIENT_SECRET` in `.env`

## Architecture

```
Browser (Svelte 5 SPA)
    |
    | HTTP/WebSocket
    v
Axum Server (Rust)
    |
    +-- SQLite + FTS5 (messages, accounts, threads, search)
    +-- IMAP (sync + IDLE push)
    +-- SMTP (send via XOAUTH2)
    +-- Ollama (local AI classification + generation)
    +-- Memories MCP (semantic search + RAG)
```

| Layer | Tech |
|-------|------|
| Backend | Rust, Axum 0.8, rusqlite (bundled), async-imap, lettre |
| Frontend | Svelte 5, TypeScript, Vite 7, Tailwind CSS 4, svelte-spa-router |
| Database | SQLite with FTS5 full-text search |
| AI | Anthropic, OpenAI, Ollama (multi-provider), Memories v5 (semantic search) |
| Auth | OAuth2 (Gmail/Outlook), session tokens, API keys |
| Deploy | Docker Compose with Ollama sidecar |

## Project Structure

```
src/
  ai/          # Ollama client, AI pipeline, Memories client
  api/         # REST endpoints (messages, search, chat, compose, agents, ...)
  auth/        # OAuth2 flows
  db/          # SQLite pool, migrations
  imap/        # IMAP sync + IDLE
  models/      # Data models (account, message, draft, ...)
  smtp/        # SMTP send
  ws/          # WebSocket hub + handlers
web/
  src/
    lib/       # API client, stores, components
    pages/     # Inbox, ThreadView, Search, Settings
    components/# UI components (ComposeModal, ChatPanel, ...)
migrations/    # SQL migration files
tests/         # Integration tests
docs/          # Design docs, research, plans
```

## Testing

```bash
# Run all tests (unit + integration)
cargo test

# Run only integration tests
cargo test --test api_integration
```

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `PORT` | `3000` | Server port |
| `DATABASE_URL` | `./data/iris.db` | SQLite database path |
| `OLLAMA_URL` | `http://localhost:11434` | Ollama API URL |
| `MEMORIES_URL` | `http://localhost:8900` | Memories MCP URL |
| `PUBLIC_URL` | `http://localhost:3000` | Public URL (for OAuth callbacks) |
| `GMAIL_CLIENT_ID` | | Gmail OAuth2 client ID |
| `GMAIL_CLIENT_SECRET` | | Gmail OAuth2 client secret |
| `OUTLOOK_CLIENT_ID` | | Outlook OAuth2 client ID |
| `OUTLOOK_CLIENT_SECRET` | | Outlook OAuth2 client secret |
| `MEMORIES_API_KEY` | | Memories MCP API key |
| `IRIS_AUTH_PASSWORD_HASH` | | Optional Argon2 password hash for the web UI login gate |
| `IRIS_SECRETS_KEY` | | Optional 32-byte base64 or 64-character hex key for encrypting persisted secrets at rest |
| `SESSION_TOKEN_FILE` | | Write session token to file (for Docker/scripts) |
| `ANTHROPIC_API_KEY` | | Anthropic API key (also configurable via Settings UI) |
| `OPENAI_API_KEY` | | OpenAI API key (also configurable via Settings UI) |
| `IRIS_CORS_ORIGINS` | `localhost:1420,5173` | Comma-separated CORS origins (set in production) |
| `BIND_ALL` | | Bind to 0.0.0.0 (required inside Docker containers) |

## License

TBD
