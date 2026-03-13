---
id: fh-018
type: feature-handoff
audience: internal
topic: Key Topics UI
status: draft
generated: 2026-03-13
source-tier: direct
hermes-version: 1.0.1
---

# FH-018: Key Topics UI

## What It Does

Key Topics surfaces AI-generated conversation topic analysis and contact statistics for individual contacts. When a user clicks a sender's name in the inbox or thread view, a popover displays the topics most frequently discussed with that contact, a count of emails exchanged, and a top-contacts list. The feature helps users quickly understand their relationship history with a contact without searching through emails manually.

Two endpoints back this feature:
- **Contact Topics**: Per-contact topic analysis with caching
- **Top Contacts**: Ranked list of most-emailed contacts

## How It Works

### Contact Topics

**Endpoint**: `GET /api/contacts/{email}/topics`

The `{email}` path parameter must be a valid email address (must contain `@`). Addresses exceeding 320 characters return 400 (RFC 5321 maximum).

**Processing sequence**:
1. Route handler (`get_contact_topics` in `src/api/contacts.rs`) validates the email address format.
2. Checks the `contact_topics_cache` table for an existing entry with `created_at` within the last 1 hour (3600 seconds).
3. **Cache hit**: Returns the cached result immediately. The response includes `"cached": true`.
4. **Cache miss**: Fetches all messages exchanged with the contact from the `messages` table, constructs an AI prompt asking for topic categorization, sends to `ProviderPool`, and parses the response.
5. Writes the result to `contact_topics_cache` with the current timestamp.
6. Returns the result with `"cached": false`.

**Response structure**:
```json
{
  "email": "sarah@example.com",
  "topics": [
    {"name": "Budget Planning", "count": 12},
    {"name": "Q4 Review",      "count": 7},
    {"name": "Team Hiring",    "count": 4}
  ],
  "total_emails": 34,
  "cached": false
}
```

Topics are sorted by `count` descending. The number of topics returned depends on AI output — typically 3–8 topics per contact.

**Cache table**: `contact_topics_cache` (migration `022_contact_topics_cache.sql`)

| Column | Type | Notes |
|---|---|---|
| `email` | TEXT | Contact email address (primary key) |
| `topics_json` | TEXT | JSON-serialized topic array |
| `total_emails` | INTEGER | Email count at time of cache write |
| `created_at` | TEXT | ISO-8601 UTC timestamp |

**Cache TTL**: 1 hour (hardcoded). There is no manual cache invalidation endpoint in the current release.

### Top Contacts

**Endpoint**: `GET /api/contacts/top`

Returns the contacts the user has exchanged the most emails with, sorted by email count descending. No query parameters required. The list is computed fresh on each call from the `messages` table — it is not cached.

**Response structure**:
```json
{
  "contacts": [
    {"email": "sarah@example.com",  "count": 34},
    {"email": "james@corp.com",     "count": 19},
    {"email": "team@lists.org",     "count": 11}
  ]
}
```

The endpoint returns up to 10 contacts (server-side limit).

**Implementation files**:
- `src/api/contacts.rs` — `get_contact_topics`, `get_top_contacts`
- `web/src/components/contacts/ContactPopover.svelte` — popover UI triggered from sender name click

## User-Facing Behavior

- Sender names in the inbox row and in the ThreadView message header are rendered as interactive elements. Clicking a sender name opens the `ContactPopover.svelte` popover.
- The popover loads immediately and shows a spinner while fetching. For cached responses the content appears in under 100ms. For uncached AI analysis, the user waits 2–8 seconds.
- The popover displays:
  - Contact email address
  - Total email count with this contact
  - Topic pills (each pill shows topic name and occurrence count)
  - A "Top Contacts" section showing up to 10 frequent contacts (always fresh)
- Clicking outside the popover closes it. No interaction persists to the compose window or any other surface.

## Configuration

Topic analysis requires a configured AI provider (Anthropic, OpenAI, or Ollama). If no provider is available when a cache miss occurs, the endpoint returns 503. The top-contacts endpoint does not require AI — it is always available.

Cache TTL of 1 hour is hardcoded in `src/api/contacts.rs`. Changing it requires a code change and rebuild.

## Error Responses

| HTTP Status | Condition |
|---|---|
| 400 | Email path parameter does not contain `@` |
| 400 | Email path parameter exceeds 320 characters |
| 503 | Cache miss and AI provider unavailable (topics endpoint only) |
| 500 | Database error or AI response parse failure |

The top-contacts endpoint (`/api/contacts/top`) does not use AI and will not return 503.

## Edge Cases & Limitations

- **Cache staleness**: New emails received after a cache entry is written will not be reflected in topic analysis until the 1-hour TTL expires. A contact with a new email thread may show outdated topic counts until the next cache miss triggers re-analysis.
- **Cache key is email address only**: Topics are computed across all accounts. If the user has two Iris accounts and both have history with the same contact, all messages from both accounts feed into the single cache entry. This is intentional for a unified contact view.
- **Empty history**: If the contact email exists in a message `From` or `To` field but the messages table has no body content, the AI may return an empty topic list. The response is valid (`"topics": []`) rather than an error.
- **Topic naming inconsistency**: Topic names are AI-generated free text. The same conversation topic may be labeled differently on consecutive cache misses ("Budget Planning" vs "Q4 Budget Review"). There is no normalization or deduplication of topic names across cache refreshes.
- **AI provider required for topics**: If the user has no AI provider configured, the topics endpoint will always return 503 on a cache miss. Once a cache entry exists (e.g., created when a provider was configured), it will be returned for up to 1 hour even after the provider is removed.
- **Top contacts limit**: The server returns at most 10 contacts. This limit is hardcoded. Distribution list addresses and no-reply addresses may appear in the list if they have high email volumes — there is no filtering.
- **No pagination on top contacts**: The `/api/contacts/top` endpoint always returns exactly one page of up to 10 results.

## Common Questions

**Q: Why is the first topics request slow?**
The first request for a contact triggers live AI analysis of all messages to/from that address. Depending on the provider, model, and message count, this can take 2–10 seconds. All subsequent requests within 1 hour return the cached result near-instantly.

**Q: How do I force a cache refresh?**
There is no API endpoint or UI control to invalidate the cache manually. The only way to force re-analysis is to wait for the 1-hour TTL to expire. For support escalations requiring fresh data, a direct SQL delete from `contact_topics_cache WHERE email = '...'` followed by a new request will trigger immediate re-analysis.

**Q: Why does the top contacts list include mailing list addresses?**
The top-contacts query counts all emails where the address appears in `From` or `To` fields. Mailing lists and automated senders (no-reply@, noreply@, etc.) are not filtered. High-volume automated email will appear in the list. There is no allowlist or blocklist in the current release.

**Q: Do topics persist if I switch AI providers?**
Yes. Once a cache entry is written, the stored JSON is returned regardless of the current provider state. The provider is only needed for new cache misses.

**Q: Is the topic count the number of emails or the number of times a topic was mentioned?**
The `count` field in a topic object represents the number of emails the AI attributed to that topic in the contact's message history. It is not a keyword occurrence count — the AI groups emails thematically and reports how many emails fall into each group.

## Troubleshooting

| Symptom | Likely Cause | Resolution |
|---|---|---|
| Returns 400 for a valid-looking email | Missing `@` in URL encoding, or email exceeds 320 chars | Check that the email is URL-encoded correctly in the path parameter |
| Returns 503 on first load | No AI provider configured | Set up at least one AI provider in Settings > AI |
| Topics show stale data | Cache TTL has not expired (within 1 hour of last analysis) | Wait 1 hour or delete the cache row directly: `DELETE FROM contact_topics_cache WHERE email = '...'` |
| Topics always show empty array | Contact has no emails with body content, or AI returned no topics | Verify the contact appears in the `messages` table; check AI provider logs for response content |
| Popover never appears on sender click | `ContactPopover.svelte` not mounting | Check browser console for JS errors; confirm the sender name element has the click handler wired |
| Top contacts shows unexpected addresses | Automated/mailing list addresses rank highly | Expected behavior; no filtering is applied in this release |
| 500 on topics endpoint | AI response parse failure or DB error | Check `iris-server` logs for stack trace; retry once; if persistent, check DB integrity |

## Related Links

- Backend: `src/api/contacts.rs` (`get_contact_topics`, `get_top_contacts`)
- Frontend: `web/src/components/contacts/ContactPopover.svelte`
- Migration: `migrations/022_contact_topics_cache.sql`
- Related: FH-019 (Response Time Patterns — uses same `/api/contacts/{email}/` URL namespace)
- Prior handoffs: FH-007 (AI Classification — shared ProviderPool), FH-010 (Agent API — contact data also accessible via agent routes)
