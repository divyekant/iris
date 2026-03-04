---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
---

# Feature Brief: Semantic Search

## One-Liner

Search your email by meaning, not just keywords. Ask for "emails about the Q4 budget" and find them — even if those exact words never appear.

## What It Is

Iris combines traditional full-text search (SQLite FTS5) with semantic vector search powered by a local Memories store. Type a natural language query, and Iris finds emails that match the intent behind your words — not just the literal text. Toggle between keyword and semantic modes depending on what you need.

## Who It's For

- Anyone who has ever searched their email and come up empty despite knowing the message exists
- Professionals who deal with varied terminology across teams, clients, and industries
- Power users who want search that understands context, synonyms, and related concepts

## The Problem It Solves

Traditional email search is literal. If you search for "budget" but the email says "financial plan," you won't find it. Gmail and Outlook have improved keyword matching, but they still rely on cloud processing and don't understand meaning. Iris brings true semantic understanding to email search — locally.

## Key Benefits

1. **Find what you mean**: Search by concept, not exact wording. "Emails about rescheduling the offsite" finds messages that say "moving the retreat to April"
2. **Dual search modes**: Switch between fast keyword search (FTS5) and deep semantic search with a single toggle
3. **Local vector store**: Semantic search runs through a local Memories instance. No email content is sent to external search services
4. **Instant keyword fallback**: If semantic search isn't configured, Iris automatically falls back to full-text search with snippet highlighting
5. **Filter and refine**: Combine semantic queries with filters like date ranges, attachment status, and account selection

## How It Works

When emails arrive, Iris indexes them in two ways: traditional full-text indexing for fast keyword search, and vector embeddings stored in a local Memories instance for semantic search. When you search, Iris sends your query to both systems and merges the results. Semantic matches surface emails that are conceptually related to your query, even when the vocabulary differs.

## Competitive Context

| Capability | Iris | Gmail | Superhuman | Shortwave | Zero |
|---|---|---|---|---|---|
| Semantic search | Local | Cloud | Cloud | Cloud (Google) | Cloud |
| Full-text search | Yes | Yes | Yes | Yes | Yes |
| Data stays local | Yes | No | No | No | No |
| Custom AI models | Yes | No | No | No | No |
| Cross-provider | Yes | Gmail only | Gmail only | Gmail only | Gmail only |
| Filter refinement | Yes | Yes | Limited | Yes | Limited |

## Proof Points

- FTS5 full-text search with snippet highlighting and relevance ranking
- Semantic search via local Memories vector store with automatic fallback
- Search works across all connected accounts simultaneously
- Filter chips for has:attachment, date ranges, and more

## Suggested Messaging

**Announcement**: "Iris now supports semantic search — find emails by meaning, not just keywords, powered entirely by local AI."

**Sales pitch**: "How many important emails have you missed because you searched the wrong keyword? Iris understands what you mean, not just what you type — and it does it without sending your data anywhere."

**One-liner**: "Email search that understands what you mean."
