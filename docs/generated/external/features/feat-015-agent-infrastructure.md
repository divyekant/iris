---
status: draft
generated: 2026-03-15
source-tier: direct
hermes-version: 1.0.0
id: feat-015
feature: agent-infrastructure
audience: external
type: feature-doc
---

# Iris CLI & Enhanced MCP Server

Iris now ships with a native command-line tool and a significantly expanded MCP server. Use the CLI to access your inbox, run searches, and chat with AI from any terminal. Build agents and automations using the full MCP tool library — now 18 tools across reading, writing, AI analysis, and bulk operations.

## Getting Started with the CLI

### 1. Install and configure

After building or installing the `iris` binary, run:

```bash
iris init --url http://localhost:3030 --key YOUR_API_KEY
```

This creates `~/.iris/config.toml` with your server URL and API key. All subsequent commands use these settings automatically — no need to pass `--url` or `--key` on every call.

Verify the connection:

```bash
iris status
```

You should see something like:

```
Iris v0.1.0 — healthy
  database   ok
  ai         ok (anthropic)
  memories   ok
```

### 2. Check your inbox

```bash
iris inbox --limit 10
```

Output:

```
  # From                    Subject                         Date
  1 Alice Chen              Re: Q3 budget review            Mar 15 09:41  [unread]
  2 Dave Kim                Action required: contract       Mar 14 17:22  [unread]
  3 Carol Wu                Lunch Thursday?                 Mar 14 11:05
  ...
```

### 3. Search your email

```bash
iris search "invoice March"
```

Pipe to JSON for scripting:

```bash
iris search "invoice March" --json | jq '.[].subject'
```

### 4. Chat with AI

Ask natural language questions about your email:

```bash
iris chat "What are my most urgent action items today?"
iris chat "Summarize the thread about the Q3 budget"
iris chat "Draft a reply to Alice's latest message"
```

Use `--quiet` to get just the response text (useful for scripts):

```bash
iris chat "How many unread emails do I have?" --quiet
```

## Full Subcommand Reference

| Subcommand | Description |
|-----------|-------------|
| `iris init` | Configure CLI with server URL and API key |
| `iris status` | Show server health and component status |
| `iris inbox` | List inbox threads |
| `iris search <query>` | Search email by full text |
| `iris send` | Send an email |
| `iris chat <prompt>` | Chat with the AI assistant |
| `iris key` | Manage API keys (list, create, revoke) |

### Common flags

| Flag | Description |
|------|-------------|
| `--json` | Output JSON instead of formatted text |
| `--quiet` | Suppress headers and decorations; emit only core content |
| `--limit N` | Limit results (default: 20) |
| `--account NAME` | Restrict to a specific email account |
| `--config PATH` | Use a custom config file path |

### Examples

```bash
# Get 5 inbox entries as JSON
iris inbox --limit 5 --json

# Search and count results
iris search "budget" --json | jq length

# Send a quick message
iris send --to bob@example.com --subject "Quick update" --body "Done."

# Create a read-only API key
iris key create --name monitoring-bot --perms read

# List all API keys
iris key list

# Revoke a key
iris key revoke --name monitoring-bot
```

## MCP Tool Reference (for Agent Developers)

Iris implements the MCP (Machine-readable Communication Protocol) server at `/api/mcp/`. Use it to build agents, LLM integrations, and automations that read and act on email.

### Starting an MCP session

```bash
# 1. Get a session token (web context)
curl -s http://localhost:3030/api/auth/bootstrap \
  -H "Sec-Fetch-Site: same-origin" | jq .token

# Or authenticate with an API key directly
TOKEN="your_session_token"

# 2. Initialize an MCP session
SESSION=$(curl -s -X POST http://localhost:3030/api/mcp/initialize \
  -H "X-Session-Token: $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"client_name":"my-agent","client_version":"1.0"}' | jq -r .session_id)
```

### Calling a tool

```bash
curl -s -X POST http://localhost:3030/api/mcp/tools/call \
  -H "X-Session-Token: $TOKEN" \
  -H "Content-Type: application/json" \
  -d "{
    \"session_id\": \"$SESSION\",
    \"tool_name\": \"list_threads\",
    \"arguments\": {\"unread\": true, \"limit\": 10}
  }"
```

### Complete tool list (18 tools)

#### Reading tools
| Tool | Description |
|------|-------------|
| `list_threads` | List threads with filters: `unread`, `starred`, `category`, `date_from`, `date_to`, `sender`, `limit` |
| `search_emails` | Full-text search with filters: `query`, `sender`, `date_from`, `date_to`, `is_read`, `limit` |
| `read_email` | Get full message content by ID |
| `get_thread_summary` | AI-generated summary for a thread (async) |
| `get_contact_profile` | Contact metadata: frequency, last date, key topics |
| `get_inbox_stats` | Aggregate counts: total, unread, starred, by category |

#### AI analysis tools
| Tool | Description |
|------|-------------|
| `extract_tasks` | Action items from a thread (async) |
| `extract_deadlines` | Date/deadline mentions in a thread (async) |
| `chat` | Run the full AI chat agent with tool-calling (async) |

#### Writing tools
| Tool | Description |
|------|-------------|
| `compose_draft` | Create a new draft |
| `manage_draft` | Update or delete a draft by ID |
| `send_email` | Send a draft or compose inline |
| `bulk_action` | Archive, mark-read, star, or delete multiple messages |

#### Session tools
| Tool | Description |
|------|-------------|
| `list_sessions` | List active MCP sessions |
| `get_session_history` | Tool call history for a session |
| `delete_session` | Close and clean up a session |

### Async tools

AI tools (`get_thread_summary`, `extract_tasks`, `extract_deadlines`, `chat`) return immediately with a `job_id`:

```json
{"status": "pending", "job_id": "abc-123"}
```

Poll for the result:

```bash
curl -s http://localhost:3030/api/mcp/jobs/abc-123 \
  -H "X-Session-Token: $TOKEN"
# Returns {"status":"complete","result":{...}} when done
```

### Example MCP session

```bash
# List unread threads
curl -s -X POST http://localhost:3030/api/mcp/tools/call \
  -H "X-Session-Token: $TOKEN" -H "Content-Type: application/json" \
  -d "{\"session_id\":\"$SESSION\",\"tool_name\":\"list_threads\",\"arguments\":{\"unread\":true}}"

# Get inbox counts
curl -s -X POST http://localhost:3030/api/mcp/tools/call \
  -H "X-Session-Token: $TOKEN" -H "Content-Type: application/json" \
  -d "{\"session_id\":\"$SESSION\",\"tool_name\":\"get_inbox_stats\",\"arguments\":{}}"

# Archive two messages
curl -s -X POST http://localhost:3030/api/mcp/tools/call \
  -H "X-Session-Token: $TOKEN" -H "Content-Type: application/json" \
  -d "{\"session_id\":\"$SESSION\",\"tool_name\":\"bulk_action\",\"arguments\":{\"action\":\"archive\",\"message_ids\":[\"msg1\",\"msg2\"]}}"

# Get AI summary for a thread (async)
JOB=$(curl -s -X POST http://localhost:3030/api/mcp/tools/call \
  -H "X-Session-Token: $TOKEN" -H "Content-Type: application/json" \
  -d "{\"session_id\":\"$SESSION\",\"tool_name\":\"get_thread_summary\",\"arguments\":{\"thread_id\":\"thread_abc\"}}" \
  | jq -r .job_id)

# Poll until complete
curl -s http://localhost:3030/api/mcp/jobs/$JOB -H "X-Session-Token: $TOKEN"
```

## API Key Permissions

Different operations require different permission levels on the API key used:

| Permission | Grants access to |
|-----------|-----------------|
| `read` | inbox, search, status, AI reads (summary, tasks, deadlines, chat) |
| `send` | compose, send email |
| `write` | draft management, bulk actions, marking read/starred |
| `admin` | API key management |
