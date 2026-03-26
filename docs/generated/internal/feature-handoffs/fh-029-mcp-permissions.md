---
id: fh-029
type: feature-handoff
audience: internal
topic: mcp-permissions
status: current
generated: 2026-03-26
source-tier: direct
hermes-version: 1.0.1
---

# Feature Handoff: MCP Permissions

## What It Does

The MCP (Model Context Protocol) server in Iris exposes email operations as callable tools for external AI models. With this update, every MCP tool call is permission-checked against the calling agent's API key. API keys can create MCP sessions, and each tool maps to a minimum permission level. This prevents a read-only agent from sending emails through the MCP layer even though the tool technically exists.

The MCP server provides 16 tools covering search, read, draft, send, threading, AI summaries, contact profiles, task/deadline extraction, chat, inbox stats, and bulk actions. Sessions are scoped to a single email account and tracked in the database with full call history.

## How It Works

### Session Creation (`POST /api/mcp/initialize`)

When an agent (or UI session) creates an MCP session:

1. The unified auth middleware extracts the `AuthContext` (Session or Agent).
2. For agent keys, the `api_key_id` is derived from the auth context (not the request body) and stored on the session.
3. The target `account_id` is validated against active accounts.
4. A session ID is generated (`mcp-{timestamp}-{uuid12}`).
5. Optional `capabilities` filter which tools are available (empty = all tools).
6. The session is stored in `mcp_sessions` with `is_active = 1`.

### Tool-to-Permission Mapping (`tool_permission()`)

Every tool call runs through `tool_permission()` which returns the minimum `Permission` required:

| Permission Level | Tools |
|---|---|
| `ReadOnly` | `search_emails`, `read_email`, `list_inbox`, `list_threads`, `get_thread`, `get_thread_summary`, `get_contact_profile`, `get_inbox_stats`, `extract_tasks`, `extract_deadlines` |
| `DraftOnly` | `create_draft`, `manage_draft`, `archive_email`, `star_email`, `bulk_action` |
| `SendWithApproval` | `send_email`, `chat` |
| `Autonomous` | Any unknown tool (fallback -- rejected by tool existence check anyway) |

### Permission Check Flow (`POST /api/mcp/tools/call`)

When a tool call arrives:

1. Session is looked up by `session_id` and verified as active.
2. `last_active_at` is updated on the session.
3. `tool_permission()` resolves the minimum permission for the requested tool.
4. `auth.require(needed)` checks the caller's auth context:
   - `AuthContext::Session` always passes.
   - `AuthContext::Agent` checks `permission.satisfies(needed)`.
5. If denied, the response is an MCP-formatted error (not HTTP 403):
   ```json
   {
     "tool_name": "send_email",
     "result": {"error": "permission denied: tool 'send_email' requires 'send_with_approval' permission"},
     "status": "permission_denied",
     "duration_ms": 0
   }
   ```
6. If allowed, the tool executes (synchronous for DB-only tools, async for AI-dependent tools).
7. The call is recorded in `mcp_tool_calls` with input, output, status, and duration.

### Tool Categories

Tools are split into sync and async execution:

- **Synchronous** (DB-only): `search_emails`, `read_email`, `list_inbox`, `send_email`, `create_draft`, `manage_draft`, `list_threads`, `get_thread`, `archive_email`, `star_email`, `get_contact_profile`, `get_inbox_stats`, `bulk_action`
- **Asynchronous** (AI-dependent): `get_thread_summary`, `extract_tasks`, `extract_deadlines`, `chat`

### Session Management

- `GET /api/mcp/tools/list` -- Returns all available tool schemas.
- `GET /api/mcp/sessions` -- Lists active sessions.
- `DELETE /api/mcp/sessions/{session_id}` -- Deactivates a session.
- `GET /api/mcp/sessions/{session_id}/history` -- Returns tool call history for the session.

## User-Facing Behavior

- An agent with `read_only` permission can search, read emails, and get AI summaries, but cannot draft, send, or modify messages through MCP.
- Permission denials appear as tool-level errors in the MCP response, not HTTP-level errors. This aligns with MCP protocol expectations where the transport succeeds but the tool reports an error.
- The UI session (browser) has full access to all MCP tools since `AuthContext::Session` bypasses permission checks.
- Tool call history is queryable per session, showing input arguments, output, status, and execution duration.

## Configuration

| Setting | Source | Default | Description |
|---|---|---|---|
| Session capabilities | `InitializeRequest.capabilities` | `[]` (all tools) | Whitelist of tool names available in this session |
| Account scope | `InitializeRequest.account_id` | (required) | Scopes the session to one email account |
| API key permission | `api_keys.permission` | (set at key creation) | Controls which tools the key can invoke |

## Edge Cases & Limitations

- **Permission denied returns MCP-format, not HTTP 403.** The HTTP status is 200 with `status: "permission_denied"` in the body. MCP clients should check the `status` field.
- **Session is scoped to one account.** There is no cross-account session. Creating a session for account A and calling a tool that references account B's messages will fail with "message not in scope."
- **Unknown tools default to Autonomous.** The `tool_permission` fallback to `Autonomous` is a safety net. The subsequent tool existence check rejects unknown tools anyway.
- **`chat` requires `SendWithApproval`.** Even though chat does not send email, it uses AI providers that could trigger tool-use actions. This is a conservative default.
- **Capabilities filter is static per session.** Once a session is created with a capability whitelist, it cannot be modified without creating a new session.
- **Session deactivation is soft delete.** Setting `is_active = 0` preserves the session record and call history for auditing.

## Common Questions

**Q: Can an API key create MCP sessions?**
A: Yes. Any authenticated request (session or API key) can create an MCP session. The API key's permission level is enforced on each tool call, not at session creation.

**Q: What happens if I call a tool not in the session's capabilities list?**
A: The tool simply is not in the session's tool list, so it cannot be called. The tool call will fail with "unknown tool."

**Q: Why does `chat` need `SendWithApproval` instead of `ReadOnly`?**
A: The chat tool can invoke AI providers that may trigger actions (via the tool-use pipeline). Requiring `SendWithApproval` prevents a read-only agent from indirectly executing actions through chat.

**Q: How are tool call durations measured?**
A: Using `std::time::Instant` -- wall-clock time from before tool execution to after, in milliseconds.

**Q: Is there a rate limit per MCP session?**
A: MCP tool calls go through the standard rate limiter keyed by the auth token (session token or API key prefix). There is no separate per-session rate limit.

## Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| `permission_denied` on tool call | API key permission too low for the tool | Check the tool-to-permission table above. Upgrade the key or use a higher-permission key. |
| `session not found` on tool call | Session ID is wrong or session was deactivated | Create a new session with `POST /api/mcp/initialize`. |
| `account not found` on initialize | Account ID does not exist or is inactive | Verify the account is active via `GET /api/accounts`. |
| Tool call succeeds in browser but fails for agent | Session bypasses permission checks, agent does not | Create a key with sufficient permission for the tool. |
| `unknown tool` error | Tool name misspelled or not in session capabilities | Check `GET /api/mcp/tools/list` for available tools. |

## Related

- [fh-027-agent-platform.md](fh-027-agent-platform.md) -- Unified auth and API keys
- [fh-010-agent-api.md](fh-010-agent-api.md) -- Original agent API
- [fh-009-ai-chat.md](fh-009-ai-chat.md) -- AI chat (available as MCP `chat` tool)
