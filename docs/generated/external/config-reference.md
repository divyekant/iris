---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
---

# Configuration Reference

Iris is configured through environment variables and in-app settings. Environment variables are read at startup from a `.env` file or from the system environment. In-app settings are stored in the database and can be changed at any time through the Settings page.

## Environment Variables

Create a `.env` file in the project root by copying the example:

```bash
cp .env.example .env
```

### Server

| Variable | Type | Default | Description |
|---|---|---|---|
| `PORT` | number | `3000` | The port the Iris server listens on |
| `DATABASE_URL` | string | `./data/iris.db` | Path to the SQLite database file. Iris creates the file and parent directories if they do not exist. |
| `PUBLIC_URL` | string | `http://localhost:3000` | The publicly accessible URL of your Iris instance. Used to construct OAuth callback URLs. If you run Iris behind a reverse proxy or on a non-default port, set this accordingly. |

### AI Services

| Variable | Type | Default | Description |
|---|---|---|---|
| `OLLAMA_URL` | string | `http://localhost:11434` | URL of the Ollama API. Iris connects to this for AI classification, summarization, chat, and writing assist. |
| `MEMORIES_URL` | string | `http://localhost:8900` | URL of the Memories MCP service. Used for semantic search and email storage. Optional -- Iris works without it, falling back to keyword search. |
| `MEMORIES_API_KEY` | string | -- | API key for authenticating with the Memories service. Optional -- only needed if your Memories instance requires authentication. |

### Email Provider OAuth

These variables are required to connect Gmail or Outlook accounts via OAuth. You only need to configure the providers you plan to use.

| Variable | Type | Default | Description |
|---|---|---|---|
| `GMAIL_CLIENT_ID` | string | -- | OAuth2 client ID for Gmail. Obtained from the Google Cloud Console. |
| `GMAIL_CLIENT_SECRET` | string | -- | OAuth2 client secret for Gmail. |
| `OUTLOOK_CLIENT_ID` | string | -- | OAuth2 client ID for Outlook. Obtained from Azure AD app registration. |
| `OUTLOOK_CLIENT_SECRET` | string | -- | OAuth2 client secret for Outlook. |

**Setting up Gmail OAuth:**
1. Go to the [Google Cloud Console](https://console.cloud.google.com/).
2. Create a new project (or use an existing one).
3. Enable the **Gmail API**.
4. Go to **Credentials > Create Credentials > OAuth 2.0 Client ID**.
5. Set the application type to **Web application**.
6. Add `http://localhost:3000/auth/callback` as an authorized redirect URI.
7. Copy the Client ID and Client Secret into your `.env` file.

**Setting up Outlook OAuth:**
1. Go to the [Azure Portal](https://portal.azure.com/).
2. Navigate to **Azure Active Directory > App registrations > New registration**.
3. Set the redirect URI to `http://localhost:3000/auth/callback` (type: Web).
4. Under **Certificates & secrets**, create a new client secret.
5. Copy the Application (client) ID and the secret value into your `.env` file.

### Integration

| Variable | Type | Default | Description |
|---|---|---|---|
| `SESSION_TOKEN_FILE` | string | -- | If set, Iris writes its randomly generated session token to this file on startup. Useful for Docker or script integration where you need to authenticate API calls from outside the browser. Example: `/app/data/session-token`. |

## Example `.env` File

```bash
PORT=3000
DATABASE_URL=./data/iris.db
OLLAMA_URL=http://localhost:11434
MEMORIES_URL=http://localhost:8900
PUBLIC_URL=http://localhost:3000

# Gmail OAuth2
GMAIL_CLIENT_ID=123456789-abcdef.apps.googleusercontent.com
GMAIL_CLIENT_SECRET=GOCSPX-your-secret-here

# Outlook OAuth2
OUTLOOK_CLIENT_ID=abcdef12-3456-7890-abcd-ef1234567890
OUTLOOK_CLIENT_SECRET=your-outlook-secret

# Memories API key (if Memories requires auth)
MEMORIES_API_KEY=

# Write session token to file (for Docker/scripts)
# SESSION_TOKEN_FILE=/app/data/session-token
```

## In-App Settings

These settings are configured through the Iris web interface at **Settings** and stored in the SQLite database. They can be changed at any time without restarting the server.

### General

| Setting | Options | Default | Description |
|---|---|---|---|
| **Theme** | Light, Dark | Light | The visual theme for the Iris interface |
| **View Mode** | Traditional, Messaging | Traditional | Layout style for the inbox and threads. Traditional shows a classic email layout; Messaging shows a chat-like conversational layout. |

### AI Configuration

| Setting | Description |
|---|---|
| **Ollama URL** | Can be overridden per-instance from the default `OLLAMA_URL` environment variable |
| **AI Model** | Select which Ollama model to use for classification, summarization, chat, and writing assist (e.g., `llama3.2`, `mistral`) |
| **Enable AI** | Master toggle for all AI features. When disabled, no AI processing occurs (no classification, no summarization, no chat, no writing assist). |

### API Key Management

| Setting | Description |
|---|---|
| **Create API Key** | Create a new agent API key with a name, permission level, and optional account scope |
| **Revoke API Key** | Immediately invalidate an existing API key |

### Audit Log

| Setting | Description |
|---|---|
| **View Audit Log** | Browse the log of all agent API actions, filterable by API key |

## Docker Compose Configuration

When running with Docker Compose, some environment variables are overridden to use Docker service names:

```yaml
services:
  iris:
    environment:
      - DATABASE_URL=/app/data/iris.db
      - OLLAMA_URL=http://ollama:11434
      - MEMORIES_URL=http://memories:8900
      - PORT=3000
```

The `iris-data` volume persists the SQLite database across container restarts. The `ollama-data` volume persists downloaded AI models.

Variables from your `.env` file are also loaded by the Docker Compose service (if the file exists), so you can put your OAuth credentials there and they will be available inside the container.
