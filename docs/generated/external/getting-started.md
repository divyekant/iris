---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
---

# Getting Started with Iris

Iris is a self-hosted, AI-native email client. You run it on your own machine, connect your email accounts, and get intelligent features like automatic classification, semantic search, chat, and writing assistance -- all powered by a local AI model.

## Prerequisites

Before you begin, make sure you have the following installed:

| Requirement | Version | Purpose |
|---|---|---|
| **Rust** | 1.85+ | Backend server |
| **Node.js** | 20+ | Frontend build tooling |
| **Ollama** | Latest (optional) | Local AI model for classification, chat, and writing assist |
| **Docker** | Latest (optional) | Alternative to building from source |

Ollama is optional. Iris works as a fully functional email client without it. AI features (classification, summarization, chat, writing assist) require Ollama to be running.

## Installation

You can run Iris in two ways: building from source for local development, or using Docker Compose for a simpler setup.

### Option A: Local Development

**1. Clone the repository and configure environment variables.**

```bash
git clone https://github.com/your-org/iris.git
cd iris
cp .env.example .env
```

Open `.env` in your editor and fill in your OAuth credentials. At minimum, you need one email provider:

```bash
# For Gmail accounts
GMAIL_CLIENT_ID=your-gmail-client-id
GMAIL_CLIENT_SECRET=your-gmail-client-secret

# For Outlook accounts
OUTLOOK_CLIENT_ID=your-outlook-client-id
OUTLOOK_CLIENT_SECRET=your-outlook-client-secret
```

See the [Config Reference](config-reference.md) for all available environment variables.

**2. Build and start the backend.**

```bash
cargo build --release
cargo run --release
```

The server starts on `http://localhost:3000` by default. You should see:

```
Iris listening on 127.0.0.1:3000
```

**3. Build and start the frontend.**

In a separate terminal:

```bash
cd web
npm install
npm run dev
```

The frontend dev server starts on `http://localhost:5173` and proxies API requests to the backend.

**4. Open Iris.**

Navigate to [http://localhost:5173](http://localhost:5173) in your browser.

### Option B: Docker Compose

**1. Clone and configure.**

```bash
git clone https://github.com/your-org/iris.git
cd iris
cp .env.example .env
```

Edit `.env` with your OAuth credentials (same as above).

**2. Start the services.**

```bash
docker compose up --build
```

This starts two services:
- **iris** -- the backend server with the bundled frontend, on port 3000
- **ollama** -- the AI model runtime for classification, chat, and writing assist

**3. Open Iris.**

Navigate to [http://localhost:3000](http://localhost:3000) in your browser.

## Quick Start

Once Iris is running, follow these steps to get your inbox up and running.

### 1. Add an email account

Click **Add Account** on the setup page. You can connect:

- **Gmail** -- uses OAuth2. You will be redirected to Google to authorize access.
- **Outlook** -- uses OAuth2. You will be redirected to Microsoft to authorize access.
- **IMAP** -- for any email provider. Enter your IMAP server, port, username, and password.

After connecting, Iris begins syncing your messages. This may take a few minutes depending on the size of your inbox.

### 2. Browse your inbox

Your messages appear in the inbox, sorted by date. You can:

- Click a message to read it in the thread view
- Use category tabs (Primary, Updates, Social, Promotions) to filter messages
- Switch between traditional and messaging view modes
- Select multiple messages and apply bulk actions (archive, delete, mark read)

### 3. Search your email

Use the search bar at the top of the page. Iris supports full-text keyword search with filters:

- Filter by attachment (`has:attachment`)
- Filter by date range
- Toggle semantic search for meaning-based results (requires Memories service)

### 4. Enable AI features (optional)

If you have Ollama running:

1. Go to **Settings > AI**
2. Verify the Ollama connection shows as connected
3. Select an AI model (e.g., `llama3.2`)
4. Enable AI processing

New incoming emails will be automatically classified with priority, category, and intent labels.

## Verify Your Installation

To confirm Iris is running correctly, check the health endpoint:

```bash
curl http://localhost:3000/api/health
```

You should see a response like:

```json
{
  "status": "ok",
  "version": "0.1.0",
  "db": true,
  "ollama": true,
  "memories": false
}
```

- `status` -- `"ok"` means the database is healthy; `"degraded"` means the database is unreachable.
- `db` -- whether the SQLite database is accessible.
- `ollama` -- whether Ollama is reachable (false is fine if you are not using AI features).
- `memories` -- whether the Memories semantic search service is reachable (optional).

## Next Steps

- [Email Management](features/feat-001-email-management.md) -- connect accounts, manage your inbox
- [Search](features/feat-002-search.md) -- find emails with keywords and semantic search
- [AI Classification](features/feat-003-ai-classification.md) -- automatic email categorization
- [AI Chat](features/feat-005-ai-chat.md) -- ask questions about your emails
- [Agent API](features/feat-006-agent-api.md) -- connect external AI agents
- [Tutorials](tutorials/) -- step-by-step guides
