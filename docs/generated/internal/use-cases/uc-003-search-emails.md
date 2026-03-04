---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
use-case: search-emails
slug: uc-003-search-emails
---

# Use Case: Search Emails

## Summary

A user searches for emails using either keyword-based full-text search (FTS5) or meaning-based semantic search (Memories). The system returns matching results with highlighted snippets and supports filtering by attachment and date range.

## Actors

- **User**: The person searching their email.
- **System**: The Iris backend (FTS5 and/or Memories).

## Preconditions

- At least one account is connected and synced.
- For FTS5: Messages are in the database (FTS triggers automatically index them).
- For semantic search: Memories server is running and emails were stored during sync.

## Flow: Keyword Search (FTS5)

1. User types a query in the search bar (e.g., "quarterly budget report").
2. Frontend calls `GET /api/search?q=quarterly+budget+report`.
3. Backend wraps each term in quotes for FTS5: `"quarterly" "budget" "report"`.
4. FTS5 searches across message_id, subject, body_text, from_address, and from_name using porter stemming.
5. Results are ranked by BM25 relevance score.
6. FTS5 snippet function generates highlighted excerpts with `<mark>` tags.
7. Response includes results (with snippets), total count, and the original query.
8. Frontend displays results with highlighted matching text.
9. User clicks a result to open the message/thread view.

## Flow: Semantic Search

1. User types a query and toggles the "Semantic" chip to on.
2. Frontend calls `GET /api/search?q=quarterly+budget+report&semantic=true`.
3. Backend calls Memories hybrid search (BM25 + vector) with query, source prefix `iris/`, and result limit.
4. For each Memories result, the message ID is extracted from the source path and the full message record is looked up in the local database.
5. If Memories returns results, they are returned directly.
6. If Memories returns nothing (server unreachable or no matches), the system falls through to FTS5 automatically.
7. Frontend displays results in the same format as keyword search.

## Flow: Filtered Search

1. User types a query and applies filter chips:
   - "has:attachment" adds `has_attachment=true` to the query.
   - Date range adds `after={timestamp}` and/or `before={timestamp}`.
   - Account selector adds `account_id={id}`.
2. Backend applies these as additional WHERE clauses alongside the FTS5 MATCH.
3. Results are filtered accordingly.

## Postconditions

- User sees a list of matching emails with highlighted snippets.
- No data is modified by search operations.

## Error Scenarios

| Scenario | System Response |
|---|---|
| Empty query | Returns empty results immediately (no FTS5 query executed) |
| Malformed FTS5 query (special characters) | FTS5 prepare fails; 500 returned; logged as search query error |
| Memories unreachable during semantic search | Falls back to FTS5 silently |
| No results found | Empty results array returned; frontend shows "no results" message |

## Related Features

- fh-006-search
- fh-011-semantic-memory
