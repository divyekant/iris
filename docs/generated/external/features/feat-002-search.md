---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
---

# Search

Iris provides two search modes: keyword search powered by SQLite FTS5, and semantic search powered by the Memories vector store. Both modes return results ranked by relevance with highlighted snippets.

## Keyword Search

Keyword search is the default mode. It uses SQLite FTS5 (Full-Text Search 5) to index message subjects, sender names, sender addresses, and body text.

### How to Search

1. Click the search bar at the top of the page, or navigate to the **Search** page.
2. Type your query and press Enter.
3. Results appear ranked by relevance, with matching terms highlighted in context snippets.

### Examples

| What you want | What to type |
|---|---|
| Emails from a specific person | `alice@example.com` or `Alice` |
| Emails about a topic | `quarterly report` |
| A specific subject line | `invoice january` |
| Emails with attachments | Type your query, then enable the **has:attachment** filter chip |

### Filter Chips

Below the search bar, you can apply additional filters:

- **has:attachment** -- only show messages that have file attachments
- **Date range** -- filter to messages within a specific time window (after/before)
- **Account** -- limit results to a specific email account

Filters combine with your search query. For example, searching for `budget` with `has:attachment` enabled finds only messages about "budget" that also have attachments.

## Semantic Search

Semantic search finds messages by meaning rather than exact keywords. It is useful when you remember the concept of an email but not the specific words used.

### Prerequisites

Semantic search requires the **Memories** service to be running and reachable. Configure it with the `MEMORIES_URL` environment variable (default: `http://localhost:8900`).

### How to Use

1. Open the Search page.
2. Toggle the **Semantic** chip to enable semantic search mode.
3. Type your query using natural language.
4. Results are ranked by meaning similarity rather than keyword match.

### Examples

| Keyword search | Semantic equivalent |
|---|---|
| `flight booking confirmation` | `travel plans next week` |
| `invoice payment due` | `bills I need to pay` |
| `meeting agenda thursday` | `what's scheduled for this week` |

### Fallback Behavior

If the Memories service is not available or returns no results, Iris automatically falls back to keyword (FTS5) search. You do not need to do anything -- the fallback is transparent.

## Search API

If you are integrating with Iris programmatically, the search endpoint is:

```
GET /api/search?q={query}
```

Query parameters:

| Parameter | Type | Default | Description |
|---|---|---|---|
| `q` | string | required | Search query text |
| `has_attachment` | boolean | -- | Filter to messages with attachments |
| `after` | integer (unix timestamp) | -- | Messages after this date |
| `before` | integer (unix timestamp) | -- | Messages before this date |
| `account_id` | string | -- | Limit to a specific account |
| `semantic` | boolean | false | Use semantic search mode |
| `limit` | integer | 50 | Maximum results to return (max 500) |
| `offset` | integer | 0 | Pagination offset |

Example response:

```json
{
  "results": [
    {
      "id": "msg-abc-123",
      "account_id": "acct-xyz",
      "thread_id": "thread-456",
      "from_address": "alice@example.com",
      "from_name": "Alice",
      "subject": "Q4 Budget Report",
      "snippet": "...the <mark>quarterly</mark> <mark>report</mark> is attached...",
      "date": 1709500800,
      "is_read": true,
      "has_attachments": true
    }
  ],
  "total": 1,
  "query": "quarterly report"
}
```

Snippets include `<mark>` tags around matching terms for keyword search results.
