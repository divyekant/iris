---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
feature: semantic-memory
slug: fh-011-semantic-memory
---

# Feature Handoff: Semantic Memory

## What It Does

Semantic memory integrates the Memories MCP vector store to enable meaning-based email search and retrieval. Emails are stored as vector embeddings during sync, and can be searched using natural language queries that understand semantic similarity rather than just keyword matching.

## How It Works

### Memories Client (`src/ai/memories.rs`)

The `MemoriesClient` is an HTTP client that communicates with a Memories MCP server (default: `http://localhost:8900`). It supports:

- **Health check**: `GET /health` -- returns true if the server responds successfully (5-second timeout).
- **Upsert**: `POST /memory/upsert` -- stores or updates a single memory entry with text, source, and key.
- **Batch upsert**: `POST /memory/upsert-batch` -- stores multiple entries at once.
- **Search**: `POST /search` -- hybrid BM25+vector search with query, k (result count), hybrid flag, and optional source prefix filter.
- **Delete by source**: `POST /memory/delete-by-source` -- deletes all entries matching a source prefix.
- **Count**: `GET /memories/count` -- counts entries, optionally filtered by source prefix.

Authentication is via an optional `X-API-Key` header. The HTTP client has a 30-second default timeout.

### Email Storage During Sync

In `SyncEngine::spawn_memories_storage` (`src/imap/sync.rs`), each synced email is stored in Memories:

1. Constructs text content: `From: {name} <{email}>\nSubject: {subject}\nDate: {date}\n\n{body}`.
2. Body is truncated to 4000 characters to stay within embedding model limits.
3. Source is set to `iris/{account_id}/messages/{db_message_id}` for routing and scoping.
4. Key is set to the RFC Message-ID for deduplication.
5. Storage is fire-and-forget (spawned as a detached Tokio task). Failures are logged at debug level but do not affect sync.

### Semantic Search Integration

Two consumers use Memories search:

1. **Search endpoint** (`src/api/search.rs`): When `semantic=true` is passed, the endpoint calls `memories.search()` with the query, source prefix `iris/{account_id}/` (or `iris/` for all accounts), and the limit. Results are resolved back to local message records by extracting the message ID from the source path. Falls back to FTS5 if Memories returns nothing.

2. **AI Chat** (`src/api/chat.rs`): The chat endpoint always tries semantic search first (source prefix `iris/`, limit 10). Results are resolved to Citation objects with subject, from, and snippet. Falls back to FTS5 citations if semantic search returns nothing.

### Source Path Convention

All Iris emails use the source format: `iris/{account_id}/messages/{db_message_id}`. This allows:
- Scoping search to a specific account: `iris/{account_id}/`
- Scoping search to all Iris emails: `iris/`
- Identifying the local message from a search result by parsing the path.

## User-Facing Behavior

- In the search page, a "Semantic" toggle chip switches between FTS5 keyword search and Memories semantic search.
- In Settings, the AI section shows a Memories health indicator (green/red) based on the `/api/health` response.
- Semantic search results appear in the same format as keyword results but may surface emails that match by meaning rather than exact words.

## Configuration

| Variable | Default | Description |
|---|---|---|
| `MEMORIES_URL` | `http://localhost:8900` | Memories MCP server base URL |
| `MEMORIES_API_KEY` | (none) | Optional API key for Memories authentication |

## Edge Cases and Limitations

- Memories is an external dependency. If the server is not running, all semantic features gracefully degrade: search falls back to FTS5, chat falls back to FTS5 citations, email storage is silently skipped.
- Body truncation to 4000 characters means very long emails may not be fully represented in the vector embedding.
- The source path parsing to extract message IDs is string-based (`rsplit('/')` to get the last segment). If the source format changes, this extraction will break.
- Deleted messages remain in Memories. There is no cleanup when messages are archived, trashed, or accounts are removed.
- The health check has a 5-second timeout. In Settings, the Memories indicator may show green even if the server is slow but responsive.
- Batch upsert is available in the client but not currently used in the sync pipeline (individual upserts are used per message).
- Search results from Memories include a relevance score, but this score is not exposed to the user or used for re-ranking.

## Common Questions

**Q: What embedding model does Memories use?**
A: This depends on the Memories MCP server configuration. Iris does not control the embedding model -- it sends text and receives search results. The model is configured on the Memories server side.

**Q: How much storage does Memories use per email?**
A: This depends on the embedding model dimensions and the Memories storage backend. Iris sends up to ~4000 characters of text per email. Typical vector embeddings are 768-1536 dimensions, consuming a few KB per entry.

**Q: What happens to Memories data when I delete an account?**
A: Currently, nothing. Memories entries persist even after account deletion. To clean up, use the `delete_by_source` function with source prefix `iris/{account_id}/`.

## Troubleshooting

| Symptom | Likely Cause | Resolution |
|---|---|---|
| Memories health indicator red | Server unreachable or not started | Start the Memories server; check MEMORIES_URL |
| Semantic search returns no results | Emails not stored in Memories, or server down | Verify Memories health; check that emails were synced after Memories was started |
| Semantic search falls back to keyword | Memories returned empty results | Check Memories storage count; verify source prefix format |
| "Memories upsert skipped" in debug logs | Server unreachable during sync | Start Memories before syncing; existing messages will not be retroactively stored |
| Search results don't match expectations | Embedding model limitations | Semantic search quality depends on the Memories embedding model |

## Related Links

- Source: `src/ai/memories.rs`, `src/imap/sync.rs` (spawn_memories_storage), `src/api/search.rs`, `src/api/chat.rs`
- Health: `src/api/health.rs` (memories health check)
- Frontend: Search page semantic toggle, Settings Memories health indicator
