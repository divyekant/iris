---
id: fh-028
type: feature-handoff
audience: internal
topic: memories-v5-integration
status: current
generated: 2026-03-26
source-tier: direct
hermes-version: 1.0.1
---

# Feature Handoff: Memories v5 Integration

## What It Does

Iris integrates with the Memories v5 vector store for semantic email search, chat memory, and preference storage. The v5 integration adds three capabilities over the earlier integration: temporal search using `document_at` timestamps, graph-aware search via `graph_weight`, and email-tuned decay parameters that zero out recency and confidence weighting. These changes make email search behave correctly -- an email from 2023 about a budget topic should rank as high as one from yesterday if the content matches, and entity relationships extracted from emails can boost relevance.

## How It Works

### Client (`src/ai/memories.rs`)

The `MemoriesClient` wraps an HTTP client with configurable base URL and API key. Key operations:

- **`upsert(text, source, key, document_at)`** -- Stores a single memory entry. The `document_at` field (new in v5) sets the document's temporal anchor. For emails, this is the message's send date in ISO 8601 format (e.g., `2024-03-15T10:00:00Z`). When `document_at` is `None`, it is omitted from the request (not sent as null) via `#[serde(skip_serializing_if = "Option::is_none")]`.

- **`upsert_batch(entries)`** -- Batch upsert multiple entries. Each `UpsertEntry` carries its own `document_at`.

- **`search(query, k, source_prefix, options)`** -- Hybrid BM25+vector search. The `SearchOptions` struct includes v5 parameters:

  | Parameter | Type | Default | Purpose |
  |---|---|---|---|
  | `since` | `Option<String>` | `None` | ISO date -- only return entries with `document_at >= since` |
  | `until` | `Option<String>` | `None` | ISO date -- only return entries with `document_at <= until` |
  | `graph_weight` | `Option<f64>` | `None` | Weight for graph/entity relationships (0.0-1.0) |
  | `recency_weight` | `Option<f64>` | `None` | Weight for recency in ranking (0.0-1.0) |
  | `confidence_weight` | `Option<f64>` | `None` | Weight for confidence in ranking (0.0-1.0) |

- **`delete_by_source(source)`** -- Deletes all entries matching a source prefix.

- **`count(source)`** -- Counts entries, optionally filtered by source.

- **`health()`** -- 5-second timeout health check against `/health`.

### Docker URL Resolution

When running inside Docker (`BIND_ALL` env var is set), the client automatically rewrites `localhost` and `127.0.0.1` URLs to `host.docker.internal` so it can reach the host-side Memories server.

### Email Search Integration (`src/api/search.rs`)

The search endpoint (`GET /api/search`) supports a `semantic=true` query parameter. When enabled with a non-empty text query:

1. Constructs a source prefix based on the optional `account_id` (e.g., `iris/1/`).
2. Passes `since` and `until` from query params directly to the Memories search.
3. Sets email-tuned decay: `graph_weight=0.1`, `recency_weight=0.0`, `confidence_weight=0.0`.
4. Maps Memories results back to messages via the source key (format: `iris/{account_id}/messages/{message_id}`).
5. Falls through to FTS5 keyword search if Memories returns no results or is unreachable.

### Memory Storage (`src/jobs/worker.rs`)

The `memories_store` job type processes each new email:

1. Extracts the message text and metadata.
2. Constructs a source key: `iris/{account_id}/messages/{message_id}`.
3. Sets `document_at` from the email's send date.
4. Upserts into Memories.

The job worker also handles:
- `chat_summarize` -- Stores chat session summaries at `iris/chat/sessions/{session_id}`.
- `pref_extract` -- Stores user preferences extracted from AI feedback.
- `entity_extract` -- Triggers knowledge graph entity extraction.
- `style_extract` -- Triggers writing style analysis.

### Config Hot-Reload

The `MemoriesClient` supports runtime config changes via `update_config(base_url, api_key)`. When the user updates Memories settings in the UI, the client picks up the new connection details without a server restart.

## User-Facing Behavior

- Semantic search in the UI uses the Memories hybrid engine. Users see results ranked by meaning, not just keyword match.
- The `since` and `until` query parameters enable temporal filtering: "show me emails from Q1 2025 about budget."
- If Memories is down, search silently falls back to keyword (FTS5). No error is shown.
- The health endpoint reports `memories: true/false` so users can verify connectivity.

## Configuration

| Setting | Location | Default | Description |
|---|---|---|---|
| Base URL | Config/env | `http://localhost:8900` | Memories server URL |
| API key | Config/env | `None` | Optional API key for Memories authentication |
| Search source prefix | Derived | `iris/{account_id}/` | Scopes search to a specific account |
| Graph weight | Hardcoded in search | `0.1` | Low graph weight for email search |
| Recency weight | Hardcoded in search | `0.0` | Zero -- email date is not correlated with relevance |
| Confidence weight | Hardcoded in search | `0.0` | Zero -- all emails are equally trustworthy |

## Edge Cases & Limitations

- **Recency weight is intentionally zero.** Standard Memories search penalizes old entries, which makes no sense for email. An email from 2020 about a specific project is just as relevant as one from today.
- **Graph weight is low (0.1).** Entity relationships provide a small boost but do not dominate ranking. This is appropriate for email where the text content is the primary signal.
- **`document_at` is optional.** Entries without it still work but cannot participate in temporal filtering. Legacy entries stored before v5 will not appear in `since/until` filtered searches.
- **Source prefix format is fixed.** Changing the format (`iris/{account_id}/messages/{message_id}`) would orphan existing entries.
- **Batch upsert returns a count.** If some entries fail silently, the returned `stored` count may be less than the input count. No per-entry error detail is available.
- **30-second client timeout.** Slow Memories responses will time out. The health check uses a separate 5-second timeout.

## Common Questions

**Q: Why is recency weight zero for email search?**
A: Because email relevance is topic-based, not time-based. A three-year-old email about a specific contract is more relevant to a "contract" search than yesterday's meeting reminder. The `since/until` parameters provide explicit temporal filtering when the user wants it.

**Q: How does `document_at` differ from the upsert timestamp?**
A: `document_at` is the email's original send date. The upsert timestamp is when Iris stored the entry (could be days later during initial sync). Temporal search uses `document_at`, not the storage time.

**Q: What happens if Memories is reconfigured to a new instance?**
A: The `update_config` method changes the URL and key at runtime. Existing entries on the old instance are not migrated -- they need to be re-indexed by re-syncing emails or running a bulk re-process.

**Q: Can I search across all accounts?**
A: Yes. Omit the `account_id` query parameter and the source prefix becomes `iris/`, which matches all accounts.

**Q: What format does `since`/`until` expect?**
A: ISO 8601 date strings. Both `YYYY-MM-DD` and full timestamps work. The Memories server handles parsing.

## Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| Semantic search returns no results | Memories server unreachable | Check `GET /api/health` for `memories: false`. Verify the Memories URL and port. |
| Temporal filtering returns empty | Entries stored without `document_at` | Re-sync affected messages to re-upsert with timestamps. |
| Search returns results from wrong account | Source prefix not set or `account_id` param missing | Pass `account_id` to scope the search. |
| Batch upsert reports fewer stored than sent | Individual entries rejected by Memories (e.g., duplicate keys) | Check Memories server logs for rejection reasons. |
| Docker container cannot reach Memories | `BIND_ALL` not set | Set `BIND_ALL=1` in Docker env to trigger localhost-to-host.docker.internal rewrite. |

## Related

- [fh-011-semantic-memory.md](fh-011-semantic-memory.md) -- Original Memories integration (pre-v5)
- [fh-006-search.md](fh-006-search.md) -- Search infrastructure (FTS5 + semantic)
- [fh-013-job-queue.md](fh-013-job-queue.md) -- Job queue (memories_store job type)
- [fh-030-showcase-features.md](fh-030-showcase-features.md) -- Knowledge graph (uses graph_weight)
