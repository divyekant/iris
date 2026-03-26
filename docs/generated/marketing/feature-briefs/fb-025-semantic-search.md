---
id: fb-025
type: feature-brief
audience: marketing
topic: semantic-search-v5
status: current
generated: 2026-03-26
hermes-version: 1.0.1
---

# Semantic Search v5

## One-Liner

Search your email by meaning — "Find Sarah's email about the budget from last week" just works.

## The Problem

Email search is still stuck on exact keywords. You know the email exists, you remember what it was about, but you can't find it because you're guessing at the right words. Traditional search punishes you for not remembering whether it said "budget," "financials," or "Q4 spending."

## The Solution

Iris Semantic Search v5 understands what you mean, not what you type. It matches by concept, filters by date range, and uses graph-aware ranking to automatically surface related context. Natural language queries return the right results on the first try.

## Key Benefits

- **Search by meaning**: Describe what you're looking for in plain language and Iris finds it, regardless of the exact words used in the email
- **Date range filtering**: Narrow results to a specific time window — "from last week," "in January," "before the deadline"
- **Graph-aware ranking**: Related emails surface automatically because Iris understands how messages connect to each other
- **Instant results**: Semantic matching runs locally against your Memories v5 vector store — no cloud round-trips
- **Natural fallback**: If semantic search isn't configured, Iris automatically uses full-text keyword search so you always get results

## How It Works (Simple)

When emails arrive, Iris creates a meaning-based index alongside the traditional keyword index. When you search, Iris matches your query against both the meaning and the words, then ranks results by relevance. Graph-aware ranking adds context — if an email is related to other important messages, it floats higher. Date filters let you zero in on exactly the right time window.

## Suggested Messaging

**Headline**: "Stop guessing keywords. Search your email the way you think about it."

**Product update**: "Semantic Search v5 in Iris matches by meaning, filters by date, and ranks by context — so 'Find Sarah's budget email from last week' returns exactly what you need."

**Comparison pitch**: "Gmail search still requires exact keywords. Iris understands what you mean. And it does it locally, without sending your queries to the cloud."

## Competitive Edge

Most email clients that offer semantic search route your queries through cloud AI services. Iris runs semantic search entirely on your machine using a local Memories v5 vector store. Graph-aware ranking is unique to Iris — no other email client understands how messages relate to each other and uses that context to improve search results. Date range filtering combined with semantic matching means you get precise, relevant results without keyword gymnastics.
