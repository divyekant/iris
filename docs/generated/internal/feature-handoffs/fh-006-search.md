---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
feature: search
slug: fh-006-search
---

# Feature Handoff: Search

## What It Does

Search provides full-text keyword search using SQLite FTS5 and optional semantic search using the Memories vector store. Users can find emails by content, filter by attachments and date ranges, and toggle between keyword and semantic search modes.

## How It Works

### FTS5 Full-Text Search

The `fts_messages` virtual table is created in migration 001 using FTS5 with `porter unicode61` tokenization. It indexes: message_id, subject, body_text, from_address, from_name. The table uses `content=messages, content_rowid=rowid` to be a content-less external content table, kept in sync by INSERT/UPDATE/DELETE triggers on the `messages` table.

The `GET /api/search` endpoint (`src/api/search.rs`):

1. Takes query parameters: `q` (search text), `has_attachment`, `after`, `before`, `account_id`, `limit` (max 500, default 50), `offset`, `semantic` (boolean toggle).
2. For FTS5 mode: wraps each search term in double quotes and joins with spaces. This performs a phrase-prefix search per term.
3. Builds a dynamic WHERE clause combining the FTS5 MATCH with optional filters (has_attachment, date range, account_id, is_deleted=0).
4. Uses FTS5 `snippet()` function for match highlighting: `snippet(fts_messages, -1, '<mark>', '</mark>', '...', 40)` returns the best matching column snippet with up to 40 tokens and `<mark>` tags for highlighting.
5. Orders by FTS5 `rank` (BM25 relevance score).
6. Runs a separate COUNT query for total matches (without LIMIT/OFFSET).

### Semantic Search

When `semantic=true` is passed:

1. The endpoint calls `state.memories.search()` with the query text, a source prefix (`iris/{account_id}/` or `iris/`), and the limit.
2. Memories performs hybrid BM25+vector search across stored email embeddings.
3. For each result, the source path is parsed to extract the message ID, and the corresponding message is looked up in the local database.
4. If Memories returns no results (server unreachable or no matches), the search falls through to FTS5 as a fallback.

### Search Response

The response includes:
- `results` -- array of `SearchResult` objects (id, account_id, thread_id, from, subject, snippet, date, is_read, has_attachments)
- `total` -- total matching count
- `query` -- the original query string

## User-Facing Behavior

- The search bar in the header accepts free-text queries.
- Search results display with highlighted matching snippets.
- Filter chips allow toggling: has:attachment, date range (after/before).
- A semantic search toggle chip switches between keyword (FTS5) and semantic (Memories) modes.
- Clicking a search result opens the message/thread view.

## Configuration

| Variable | Default | Description |
|---|---|---|
| `MEMORIES_URL` | `http://localhost:8900` | Memories MCP server for semantic search |
| `MEMORIES_API_KEY` | (none) | Optional API key for Memories authentication |

FTS5 requires no configuration -- it is built into SQLite and created by migration 001.

## Edge Cases and Limitations

- FTS5 uses `porter` stemming, which works well for English but may produce poor results for other languages.
- The FTS5 query wraps each term in quotes, which means multi-word phrases are searched as individual quoted terms (AND semantics), not as an exact phrase.
- Semantic search requires the Memories server to be running and emails to have been stored during sync. If Memories is unreachable, the fallback to FTS5 is automatic and silent.
- The snippet function returns up to 40 tokens from the best-matching column. For long emails, the snippet may not include the most relevant section.
- The `has_attachment` filter checks the `has_attachments` boolean flag. It does not search by attachment filename.
- Date range filters use unix timestamps. The frontend is responsible for converting user-friendly date inputs to timestamps.
- Maximum result limit is 500 per request.

## Common Questions

**Q: What is the difference between keyword and semantic search?**
A: Keyword search (FTS5) matches exact words and their stems. Semantic search (Memories) understands meaning and can find related content even if the exact words are not present. For example, searching "budget concerns" via semantic search can find emails about "financial worries" or "cost overruns."

**Q: How does FTS5 snippet highlighting work?**
A: The snippet function finds the section of text that best matches the query and wraps matching terms in `<mark>` tags. The frontend renders these as highlighted text.

**Q: Can I search across all accounts at once?**
A: Yes. Omit the `account_id` parameter to search across all accounts. Include it to scope results to a single account.

## Troubleshooting

| Symptom | Likely Cause | Resolution |
|---|---|---|
| Search returns no results for known content | FTS5 index out of sync or content not indexed | Check that sync completed; verify FTS triggers exist |
| Semantic toggle does nothing | Memories server unreachable; falls back to FTS5 | Check Memories health via `/api/health` |
| Slow search on large mailbox | FTS5 rank ordering is CPU-intensive | Reduce result limit; ensure WAL mode is enabled |
| Snippet shows wrong section of email | FTS5 picks the best-matching column, not best section | This is by design; snippet approximates the best match |
| "Search query error" in logs | Malformed FTS5 query (special characters) | Check the query string for FTS5-incompatible characters |

## Related Links

- Source: `src/api/search.rs`, `src/ai/memories.rs`
- Database: `migrations/001_initial.sql` (fts_messages table, triggers)
- Frontend: Search page, filter chips, search bar component
