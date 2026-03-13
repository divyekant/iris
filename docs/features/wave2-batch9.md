# Wave 2 Batch 9: Feature Documentation

## #41 Attachment Content Search

Extracts and full-text indexes the content of email attachments so users can find emails by what's inside their files, not just the subject or body.

### API

**POST** `/api/attachments/index/{message_id}`

Triggers extraction and indexing for all attachments on the given message. Returns immediately; extraction is synchronous.

**Response:**
```json
{
  "indexed": 2,
  "skipped": 1,
  "message_id": "abc123"
}
```

---

**GET** `/api/attachments/search`

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `q` | string | required | Full-text search query |
| `account_id` | string | all | Filter to a specific account |
| `limit` | int | 20 | Max results (cap 100) |
| `offset` | int | 0 | Pagination offset |

**Response:**
```json
{
  "results": [
    {
      "message_id": "abc123",
      "filename": "Q3_Report.csv",
      "mime_type": "text/csv",
      "snippet": "...revenue grew by <b>24%</b> in Q3...",
      "account_id": "user@example.com"
    }
  ],
  "total": 5
}
```

---

**GET** `/api/attachments/search/stats`

Returns index coverage stats: total indexed attachments, total size, breakdown by MIME type.

---

**POST** `/api/attachments/reindex`

Clears and rebuilds the full attachment content index. Intended for maintenance or after a schema migration.

### How It Works

Attachment text is extracted on demand (via POST index) or during the sync pipeline. Extracted text is stored in the `attachment_text_cache` table (migration 046) with a corresponding FTS5 virtual table for efficient full-text search. Supported types with full extraction: `text/plain`, `text/csv`, `text/markdown`, `text/html` (tags stripped). PDF, Word, and Excel formats return a placeholder stub today; extraction stubs are in place for future library integration. Snippet highlighting uses SQLite's built-in `snippet()` function on the FTS5 table. All queries are account-scoped to prevent cross-account data leaks.

### Configuration

No additional configuration. Indexing can be triggered per-message or in bulk via the reindex endpoint. Does not require AI.

---

## #50 Thread Clustering

Groups related or duplicate email threads together using similarity analysis, reducing inbox clutter when the same conversation appears under multiple subject lines.

### API

**POST** `/api/thread-clusters/compute`

Runs the clustering algorithm over all threads and persists the result. Can be triggered on demand or scheduled.

**Request (optional):**
```json
{
  "threshold": 0.4,
  "account_id": "user@example.com"
}
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `threshold` | float | 0.4 | Jaccard similarity threshold (0.0-1.0). Lower = more aggressive grouping. |
| `account_id` | string | all | Limit clustering to one account |

**Response:**
```json
{
  "clusters_created": 12,
  "threads_clustered": 38
}
```

---

**GET** `/api/thread-clusters`

Returns all current clusters with member thread IDs, cluster size, and representative subject.

**GET** `/api/thread-clusters/{id}`

Returns a single cluster with full member details.

---

**POST** `/api/thread-clusters/{id}/merge`

Manually merges another cluster into this one.

**Request:**
```json
{ "source_cluster_id": "cluster-456" }
```

---

**DELETE** `/api/thread-clusters/{id}`

Dissolves a cluster, returning all threads to unclustered state.

**DELETE** `/api/thread-clusters/{cluster_id}/members/{thread_id}`

Removes a single thread from a cluster (split operation).

### How It Works

Clustering uses Jaccard similarity on two signals: normalized subject tokens and the set of participant email addresses. Two threads with Jaccard score above the threshold are considered related. A union-find data structure handles transitive merging — if A is similar to B and B is similar to C, all three end up in the same cluster even if A and C are dissimilar directly. Results are persisted to the `thread_clusters` table (migration 047). Manual merge and split operations let users correct the algorithm's decisions.

### Configuration

The similarity threshold defaults to 0.4 and is configurable per compute request. No AI required.

---

## #71 Phishing Detection

Analyzes incoming emails for phishing signals and produces a composite risk score, giving users and agents a structured security assessment for any message.

### API

**POST** `/api/security/phishing-scan/{message_id}`

Runs the full phishing analysis on the given message and caches the result.

**Response:**
```json
{
  "message_id": "abc123",
  "risk_score": 0.78,
  "risk_level": "high",
  "signals": [
    { "name": "urgency_language", "score": 0.9, "detail": "Contains phrases: 'act now', 'expires today'" },
    { "name": "sender_mismatch", "score": 0.8, "detail": "Display name 'PayPal Support' but domain 'mail.paypa1.net'" },
    { "name": "credential_request", "score": 0.7, "detail": "Body requests password or account credentials" }
  ],
  "scanned_at": 1741564800
}
```

**Risk levels:** `low` (< 0.3), `medium` (0.3–0.6), `high` (> 0.6).

---

**GET** `/api/security/phishing-report/{message_id}`

Returns the cached phishing report for a message. Returns 404 if the message has not been scanned.

---

**GET** `/api/security/phishing-reports`

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `account_id` | string | all | Filter by account |
| `min_risk` | float | 0.0 | Minimum risk score |
| `limit` | int | 50 | Max results (cap 200) |
| `offset` | int | 0 | Pagination offset |

---

**POST** `/api/security/phishing-bulk-scan`

**Request:**
```json
{ "message_ids": ["id1", "id2", "id3"], "account_id": "user@example.com" }
```

Scans up to 50 messages in one request. Returns an array of per-message results.

---

**GET** `/api/security/phishing-stats`

Returns account-level aggregates: total scanned, breakdown by risk level, top signal triggers.

### How It Works

Seven independent rule-based signal detectors run in parallel on each message:

| Signal | What It Checks |
|--------|---------------|
| `urgency_language` | Keywords: "act now", "expires", "verify immediately", etc. |
| `suspicious_urls` | Hyperlinks where display text and href domain differ |
| `sender_mismatch` | Display name implies a known brand but domain doesn't match |
| `homograph_domains` | Domain contains mixed scripts or common glyph substitutions |
| `credential_request` | Body asks for password, SSN, credit card, or login |
| `reply_to_mismatch` | Reply-To header differs from From domain |
| `known_patterns` | Regex matches against a compiled list of phishing patterns |

Each detector returns a score (0.0–1.0). The composite score is a weighted average; weights are tuned so sender mismatch and credential requests contribute more heavily. Reports are cached in the `phishing_reports` table (migration 048) and reused on subsequent GET requests.

### Configuration

No configuration required. Does not require AI. Bulk scan is capped at 50 messages per request.

---

## #77 Contact Profiles

Generates AI-powered profiles for contacts based on email interaction history — capturing communication style, recurring topics, and relationship context.

### API

**GET** `/api/contacts/profiles`

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `account_id` | string | all | Filter by account |
| `limit` | int | 50 | Max results (cap 200) |
| `offset` | int | 0 | Pagination offset |

**Response:**
```json
{
  "profiles": [
    {
      "email": "alice@example.com",
      "name": "Alice Chen",
      "profile_summary": "Alice is a project manager who communicates concisely and typically follows up within 24 hours. Emails frequently reference sprint planning and delivery timelines.",
      "topics": ["project management", "sprint planning", "deadlines"],
      "communication_style": "direct",
      "avg_response_time_hours": 6.2,
      "total_emails": 83,
      "last_contact": 1741564800,
      "generated_at": 1741478400
    }
  ],
  "total": 14
}
```

---

**POST** `/api/contacts/profiles/generate/{email}`

Generates (or regenerates) the profile for a single contact. Reads all emails from/to that address, then calls the AI provider.

---

**POST** `/api/contacts/profiles/generate-all`

Generates profiles for all contacts with 3 or more emails. Processes contacts sequentially to avoid overloading the AI provider.

**Response:**
```json
{ "generated": 12, "skipped": 3, "failed": 1 }
```

---

**GET** `/api/contacts/profiles/search`

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `q` | string | required | Full-text search over profile summaries and topics |
| `limit` | int | 20 | Max results |

---

**GET** `/api/contacts/profiles/{email}`

Returns the full profile for a specific contact email address.

---

**DELETE** `/api/contacts/profiles/{email}`

Deletes the stored profile. The profile can be regenerated at any time.

### How It Works

Profile generation reads the most recent 50 emails to/from the contact, computes frequency and response-time stats, and sends a structured summary to the AI provider. The prompt asks the model to identify communication style, recurring topics, and a 2-3 sentence relationship summary. Profiles are stored in the `contact_profiles` table (migration 049) with a `generated_at` timestamp; they can be refreshed on demand. A minimum of 3 emails is required before a profile is generated — contacts with fewer interactions are skipped by `generate-all`.

### Configuration

Requires AI enabled. The minimum email threshold (3) is hardcoded. Profile search uses SQLite FTS5 on the `profile_summary` and `topics` columns.

---

## #81 MCP Server

Exposes Iris email as a set of Model Context Protocol (MCP) tools, allowing external AI agents and Claude-based workflows to read, search, and compose email through a standardized interface.

### API

**POST** `/api/mcp/initialize`

Creates a new MCP session and returns a session ID with TTL.

**Request:**
```json
{ "client_name": "my-agent", "client_version": "1.0" }
```

**Response:**
```json
{
  "session_id": "sess_abc123",
  "expires_at": 1741651200,
  "protocol_version": "2024-11-05",
  "server_info": { "name": "iris-mcp", "version": "1.0" }
}
```

---

**GET** `/api/mcp/tools/list`

Returns the full catalog of available tools with their JSON Schema input definitions.

**Available tools:**

| Tool | Description |
|------|-------------|
| `search_emails` | Full-text search with date, sender, and folder filters |
| `read_email` | Fetch full message content including body and headers |
| `list_folders` | List all IMAP folders for an account |
| `list_labels` | List AI-assigned category labels |
| `compose_draft` | Create a draft with to/cc/subject/body |
| `send_email` | Send an email immediately |
| `get_thread` | Retrieve all messages in a thread |
| `list_contacts` | List contacts with optional search |
| `get_inbox_stats` | Return unread counts, priority breakdown, sync status |

---

**POST** `/api/mcp/tools/call`

Invokes a named tool with the provided arguments. Requires `X-MCP-Session` header.

**Request:**
```json
{
  "tool": "search_emails",
  "arguments": {
    "query": "budget Q3",
    "date_from": "2026-01-01",
    "limit": 10
  }
}
```

**Response:**
```json
{
  "content": [
    { "type": "text", "text": "Found 4 emails matching 'budget Q3'..." }
  ],
  "is_error": false
}
```

---

**GET** `/api/mcp/sessions`

Lists all active sessions (admin/agent use).

**DELETE** `/api/mcp/sessions/{session_id}`

Terminates a session early.

**GET** `/api/mcp/sessions/{session_id}/history`

Returns the tool call history for a session, ordered by invocation time.

### How It Works

The MCP server layer is a thin adapter over Iris's existing API endpoints. Each tool call maps to the same business logic used by the Svelte frontend: `search_emails` calls the FTS5 search pipeline, `compose_draft` creates a draft via the SMTP draft handler, `send_email` invokes the lettre SMTP client. Sessions are stored in the `mcp_sessions` table (migration 050) with a configurable TTL (default 24 hours). Tool call history is appended to `mcp_tool_calls` for auditability. Authentication uses the existing API key middleware — MCP clients must present a valid `X-API-Key` header; session tokens add a second layer for per-session scoping.

### Configuration

Requires an API key with at minimum `read` permission. `send_email` and `compose_draft` require `write` permission. Session TTL is configurable in the AI config table (`mcp_session_ttl_hours`, default 24). The MCP protocol version implemented is `2024-11-05`.

---

## Summary

| Feature | File | Migration | Tests |
|---------|------|-----------|-------|
| #41 Attachment Content Search | `src/api/attachment_search.rs` | 046 | 18 |
| #50 Thread Clustering | `src/api/thread_clusters.rs` | 047 | 23 |
| #71 Phishing Detection | `src/api/phishing_detection.rs` | 048 | 19 |
| #77 Contact Profiles | `src/api/contact_profiles.rs` | 049 | 23 |
| #81 MCP Server | `src/api/mcp_server.rs` | 050 | 19 |
