---
id: feat-025
type: feature-doc
audience: external
topic: semantic-search-v5
status: current
generated: 2026-03-26
hermes-version: 1.0.1
---

# Semantic Search v5

## Overview

Iris v0.4.0 upgrades its Memories integration to support temporal search, graph-aware ranking, and email-tuned relevance scoring. You can now search for emails by meaning within a date range, and the search engine uses knowledge graph connections to surface related results you might have missed.

## Getting Started

Add `semantic=true` to any search request to activate semantic search. Combine it with `since` and `until` to narrow results to a time window.

```bash
curl -s -H "Authorization: Bearer YOUR_KEY" \
  "http://localhost:3000/api/search?q=budget+planning&semantic=true&since=2026-01-01&until=2026-03-31"
```

## How It Works

### Temporal Search

When you include `since` and/or `until` parameters, Iris passes these to the Memories search engine. Only emails whose sent date falls within the specified range are returned.

This works because Iris now stores each email's sent date (not its index time) in the Memories vector store using the `document_at` field. Searching for "emails from January" returns emails actually sent in January, even if they were synced to Iris later.

**Date format:** ISO 8601 date strings (e.g., `2026-01-01`, `2026-03-15`).

### Graph-Aware Ranking

Memories v5 maintains a knowledge graph of entities and their relationships. When you search for "budget concerns", the search engine also considers related entities -- people, projects, and organizations connected to budget-related emails -- and boosts results that are graph-adjacent to your query.

Iris passes `graph_weight: 0.1` by default, giving a mild boost to graph-connected results without overwhelming the text relevance signal.

### Email-Tuned Defaults

Email search has different relevance needs than general knowledge retrieval:

| Parameter | Default | Why |
|---|---|---|
| `recency_weight` | `0.0` | An email from six months ago about a contract is just as relevant as one from yesterday. Recency decay would bury important older emails. |
| `confidence_weight` | `0.0` | Email content has stable relevance -- it does not degrade over time like some knowledge base entries. |
| `graph_weight` | `0.1` | Mild graph expansion to surface related results without overwhelming direct matches. |

These defaults are applied automatically when you use `semantic=true`. You do not need to set them manually.

### Hybrid Search

Semantic search in Iris always runs in hybrid mode, combining:

1. **BM25 keyword matching** -- finds emails containing the exact terms you searched for
2. **Vector similarity** -- finds emails with similar meaning, even if they use different words

The results are merged and ranked. This means a search for "financial concerns" will find emails that say "budget worries" or "cost issues" even if they never use the word "financial".

### Fallback Behavior

If the Memories service is unavailable or returns no results, Iris automatically falls back to SQLite FTS5 keyword search. Your search always returns results if matching emails exist.

## Examples

### Search within a date range

```bash
curl -s -H "Authorization: Bearer YOUR_KEY" \
  "http://localhost:3000/api/search?q=project+deadline&semantic=true&since=2026-03-01&until=2026-03-31"
```

### Search for a topic across all time

```bash
curl -s -H "Authorization: Bearer YOUR_KEY" \
  "http://localhost:3000/api/search?q=quarterly+review&semantic=true"
```

### Search within a specific account

```bash
curl -s -H "Authorization: Bearer YOUR_KEY" \
  "http://localhost:3000/api/search?q=contract+renewal&semantic=true&account_id=acct-xyz&since=2026-01-01"
```

### Combine with search operators

You can mix semantic search with search operators for precise filtering:

```bash
curl -s -H "Authorization: Bearer YOUR_KEY" \
  "http://localhost:3000/api/search?q=from:sarah+budget&semantic=true&since=2026-02-01"
```

### Response format

The response is the same structure as a standard search, with results ranked by semantic relevance:

```json
{
  "results": [
    {
      "id": "msg-abc-123",
      "account_id": "acct-xyz",
      "thread_id": "thread-456",
      "from_address": "sarah@example.com",
      "from_name": "Sarah Chen",
      "subject": "Q1 Budget Review",
      "snippet": "We need to revisit the budget allocations for the engineering team...",
      "date": 1709500800,
      "is_read": true,
      "has_attachments": false
    }
  ],
  "total": 1,
  "query": "budget planning"
}
```

## Configuration

Semantic search requires the Memories service to be running and configured.

| Variable | Default | Description |
|---|---|---|
| `MEMORIES_URL` | `http://localhost:8900` | URL of the Memories service |
| `MEMORIES_API_KEY` | -- | API key for Memories authentication (if required) |

No additional configuration is needed for v5 features. The temporal and graph-aware parameters are used automatically when available.

## FAQ

**Do I need to re-index my emails for temporal search to work?**
Emails synced after the v0.4.0 upgrade automatically include the `document_at` field. Older emails indexed before the upgrade may not appear in temporal searches with `since`/`until` filters. A backfill is not required but can be done by re-syncing affected accounts.

**What happens if Memories is not running?**
Iris falls back to SQLite FTS5 keyword search. The `since` and `until` parameters are ignored in fallback mode (use the `after` and `before` query parameters for date filtering with keyword search).

**Can I control the graph weight?**
The graph weight is set to `0.1` by default and is not exposed as a query parameter in the search API. This value provides a balanced boost without overwhelming direct text matches.

**Does semantic search work with the agent API?**
Yes. With unified authentication in v0.4.0, agents can use `GET /api/search?semantic=true` with all the same parameters available to the web UI.
