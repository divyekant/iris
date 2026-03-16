---
status: draft
generated: 2026-03-15
source-tier: direct
hermes-version: 1.0.0
feature: agent-infrastructure
slug: fh-015-agent-infrastructure
id: fh-015
audience: internal
type: feature-handoff
---

# Feature Handoff: Agent Infrastructure — CLI & MCP

## What It Does

Layer 2 ships two major surfaces for programmatic and terminal access to Iris: a native `iris` CLI binary and a significantly expanded MCP server tool set. The CLI enables power users and scripts to interact with Iris from the terminal — checking inbox, searching, sending, chatting with AI, and managing API keys — without opening a browser. The MCP server gains 9 new or enhanced tools (bringing the total to 18) that expose richer query capabilities, AI-generated summaries, inbox statistics, and bulk operations to external agents and LLM tool-calling loops.

## How It Works

### CLI Binary

The CLI is compiled as a separate Rust binary (`iris-cli`) in the same workspace. It reads configuration from `~/.iris/config.toml` (URL and API key), which is created by `iris init`. Every subcommand calls the Iris REST API over HTTP using the stored key for authentication — the CLI is a thin client; all logic stays server-side.

**Full subcommand list:**

| Subcommand | Description | Example |
|-----------|-------------|---------|
| `init` | Create or update `~/.iris/config.toml` | `iris init --url http://localhost:3030 --key iris_abc` |
| `status` | Show server health and component checks | `iris status` |
| `inbox` | List inbox threads (paginated) | `iris inbox --limit 10 --account work` |
| `search` | Full-text search across messages | `iris search "Q3 budget" --limit 5` |
| `send` | Send an email from a file or stdin | `iris send --to alice@example.com --subject "Hi" --body "Hello"` |
| `chat` | Send a prompt to the AI chat agent | `iris chat "What needs my attention today?"` |
| `key` | Manage API keys (list, create, revoke) | `iris key list` / `iris key create --name ci-bot --perms read` |

**Global flags** (all subcommands):
- `--json`: emit machine-readable JSON to stdout instead of human-formatted table/text
- `--quiet`: suppress decorative output (headers, progress indicators); emit only the core result
- `--config <path>`: override the default config path (default: `~/.iris/config.toml`)
- `--url <url>`: override the server URL for this invocation (skips config)
- `--key <key>`: override the API key for this invocation (skips config)

**Config file location:** `~/.iris/config.toml`

```toml
url = "http://localhost:3030"
key = "iris_abc123"
```

The config directory (`~/.iris/`) is created automatically by `iris init` if it does not exist.

**Output modes:**
- Default (human): tabular or prose output suitable for terminal display
- `--json`: JSON array for list commands, JSON object for single-item commands
- `--quiet`: plain text only — useful for piping to other tools

### MCP Server Enhancements

The MCP server (`/api/mcp/`) retains all 9 existing tools and adds 9 new or enhanced tools. All tools support the standard `POST /api/mcp/tools/call` interface with `session_id`, `tool_name`, and `arguments`.

**New tools (9 additions):**

| Tool | Description | Key Arguments |
|------|-------------|---------------|
| `get_thread_summary` | AI-generated summary for a thread (lazy, cached) | `thread_id` |
| `get_contact_profile` | Contact metadata: email frequency, last contact, topics | `email` |
| `extract_tasks` | Action items extracted from a thread | `thread_id` |
| `extract_deadlines` | Deadline dates mentioned in a thread | `thread_id` |
| `chat` | Run the AI agentic chat loop, returns response + citations | `prompt`, optional `thread_id` |
| `get_inbox_stats` | Aggregate counts: total, unread, starred, by category | (none) |
| `manage_draft` | Create, update, or delete a draft | `action`, `draft_id`/`fields` |
| `bulk_action` | Archive, mark-read, star, delete multiple messages | `action`, `message_ids[]` |
| `list_threads` (enhanced) | Now supports 6 new filter params | `unread`, `starred`, `category`, `date_from`, `date_to`, `sender` |

**Async tool execution:** AI tools (`get_thread_summary`, `extract_tasks`, `extract_deadlines`, `chat`) run asynchronously via the job queue when an LLM generation is required. The tool call returns immediately with a `job_id` and `status: "pending"`. Clients poll `GET /api/mcp/jobs/{job_id}` or use the session history to retrieve the result once `status: "complete"`. Non-AI tools (stats, list, bulk) are synchronous and return results inline.

**Session management:** MCP sessions remain unchanged — `POST /api/mcp/initialize` creates a session, all calls include `session_id`, session history is available at `GET /api/mcp/sessions/{id}/history`, and `DELETE /api/mcp/sessions/{id}` closes a session. Sessions do not expire automatically but should be explicitly closed by agents.

## User-Facing Behavior

From a terminal, users can now:
- `iris init` once to configure, then use all subcommands without re-specifying URL/key
- Check inbox at a glance: `iris inbox --limit 5`
- Pipe JSON output to other tools: `iris search "invoice" --json | jq '.[].subject'`
- Chat with Iris AI from the terminal: `iris chat "Summarize my unread from Alice"`
- External agents and LLMs using MCP can now filter threads, fetch AI summaries, extract tasks and deadlines, query inbox statistics, and perform bulk operations — all through the standard tool-calling interface

## Configuration

**New config keys** (all optional, set via `iris init` or editing `~/.iris/config.toml`):

| Key | Default | Description |
|-----|---------|-------------|
| `url` | (required) | Iris server URL |
| `key` | (required) | API key for authentication |
| `default_limit` | 20 | Default `--limit` for inbox/search if not specified |
| `default_account` | (none) | Default account name to filter by |

No new server-side config variables. The CLI uses the existing agent auth middleware (API key → `X-Iris-API-Key` header).

## Common Questions

**Q: How does `iris chat` differ from the web ChatPanel?**
Both use the same backend agentic loop (`POST /api/ai/chat`). The CLI version passes the prompt as a single-turn message, receives the full response, and exits. It does not maintain a persistent multi-turn session across CLI invocations — each `iris chat` call is a fresh turn. For multi-turn terminal sessions, users should chain prompts or use the web UI's persistent ChatPanel.

**Q: Are AI MCP tools (get_thread_summary, extract_tasks) blocking or async?**
They are async. The tool call returns `status: "pending"` with a `job_id` immediately. The client polls `GET /api/mcp/jobs/{job_id}` until `status: "complete"`, then reads `result`. This prevents long-running AI calls from blocking the HTTP connection. Non-AI tools (list_threads, get_inbox_stats, bulk_action, manage_draft) are synchronous and return results in the tool call response.

**Q: What permissions does an API key need for each CLI subcommand?**
- `iris status`: `read` (health endpoint is open but key still required for auth)
- `iris inbox`, `iris search`: `read`
- `iris send`: `send`
- `iris chat`: `read` (chat uses read access to retrieve context)
- `iris key list/create/revoke`: `admin`
- MCP tool calls follow the same permission mapping as the direct REST equivalents

## Files Affected

- `src/bin/iris_cli.rs` — CLI binary entrypoint and command dispatch
- `src/cli/` — subcommand modules: `init.rs`, `status.rs`, `inbox.rs`, `search.rs`, `send.rs`, `chat.rs`, `key.rs`
- `src/api/mcp.rs` — new tool handlers: get_thread_summary, get_contact_profile, extract_tasks, extract_deadlines, chat, get_inbox_stats, manage_draft, bulk_action; enhanced list_threads
- `src/api/mcp_jobs.rs` — async job polling endpoint for MCP AI tools
- `Cargo.toml` — `[[bin]]` entry for `iris-cli`
- `~/.iris/config.toml` — user config (not in repo; created at runtime by `iris init`)
