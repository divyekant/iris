---
id: fh-019
type: feature-handoff
audience: internal
topic: Response Time Patterns
status: draft
generated: 2026-03-13
source-tier: direct
hermes-version: 1.0.1
---

# FH-019: Response Time Patterns

## What It Does

Response Time Patterns computes per-contact reply time statistics from actual message timestamps in the local database. The feature answers two questions: how quickly does the contact typically reply to the user, and how quickly does the user typically reply to the contact. Statistics include average, fastest, and slowest reply times, plus total exchange counts.

Results are surfaced in a `ResponseTimeCard.svelte` component that appears in the contact popover (alongside Key Topics from FH-018) and optionally in the thread detail view.

## How It Works

**Endpoint**: `GET /api/contacts/{email}/response-times`

The `{email}` path parameter must contain `@` and must not exceed 320 characters.

**Processing sequence**:
1. Route handler (`get_response_times` in `src/api/contacts.rs`) validates the email address format; returns 400 on failure.
2. Calls `compute_reply_stats` with the contact email and the current user's account addresses.
3. `compute_reply_stats` queries the `messages` table for all messages in threads where the contact participated, ordered by `date` ascending.
4. Iterates through the message sequence and identifies reply pairs by the following rule: a message counts as a reply if its sender differs from the sender of the immediately preceding message in the thread. Consecutive messages from the same sender do not count as a reply pair.
5. Separates reply pairs into two groups: "their replies" (contact replying to user) and "user replies" (user replying to contact). Computes elapsed hours between each pair.
6. Calculates averages, fastest, and slowest from each group.
7. Returns null for any metric where no valid reply pairs exist for that direction.

**Response structure**:
```json
{
  "email": "sarah@example.com",
  "their_avg_reply_hours": 3.2,
  "your_avg_reply_hours": 1.1,
  "their_reply_count": 14,
  "your_reply_count": 17,
  "fastest_reply_hours": 0.1,
  "slowest_reply_hours": 48.5,
  "total_exchanges": 31
}
```

**Field definitions**:

| Field | Type | Description |
|---|---|---|
| `their_avg_reply_hours` | float or null | Mean hours the contact took to reply to the user |
| `your_avg_reply_hours` | float or null | Mean hours the user took to reply to the contact |
| `their_reply_count` | integer | Number of reply pairs where contact replied to user |
| `your_reply_count` | integer | Number of reply pairs where user replied to contact |
| `fastest_reply_hours` | float or null | Shortest reply time across both directions |
| `slowest_reply_hours` | float or null | Longest reply time across both directions |
| `total_exchanges` | integer | Sum of `their_reply_count` + `your_reply_count` |

All float values are in hours. Null is returned when the corresponding set of reply pairs is empty (e.g., if a contact has only sent emails and never replied, `their_avg_reply_hours` will be null).

**Implementation files**:
- `src/api/contacts.rs` — `get_response_times`, `compute_reply_stats`
- `web/src/components/contacts/ResponseTimeCard.svelte` — display card with speed labels

## User-Facing Behavior

- `ResponseTimeCard.svelte` renders within the contact popover (same popover as FH-018 Key Topics), loading in parallel with topic data.
- The card displays speed category labels alongside the numeric stats:
  - **Fast**: average reply under 1 hour — green label
  - **Medium**: 1–24 hours — yellow/amber label
  - **Slow**: over 24 hours — red label
- Speed labels are applied independently to "their" and "your" average reply times.
- Fastest and slowest reply times are shown as supporting detail below the speed label.
- If reply count is zero for a direction, that row shows "No data" rather than a numeric stat.
- `total_exchanges` is shown as a summary count at the card footer.

## Configuration

No AI provider required. Response time computation is purely database-driven from existing message timestamps. The feature is always available as long as the `messages` table contains data.

No configuration flags or settings specific to this feature.

## Error Responses

| HTTP Status | Condition |
|---|---|
| 400 | Email path parameter does not contain `@` |
| 400 | Email path parameter exceeds 320 characters |
| 500 | Database query error |

There is no 404 for unknown contacts. If the email address has no messages in the database, the response returns valid JSON with all counts set to 0 and all float fields set to null.

## Edge Cases & Limitations

- **Same-sender consecutive messages are excluded**: If the user sends three emails in a row without the contact replying, those are not counted as reply pairs. Only the first message in a consecutive run from one sender matters for reply-time computation.
- **Cross-account threading**: Reply times are computed across all accounts. If the user has two Iris accounts and both exchange emails with the same contact, messages from both accounts feed into the statistics. This produces a unified view but may mix reply behavior across different email addresses the user operates from.
- **Message date accuracy**: Computation relies on the `date` field in the `messages` table, which is parsed from the `Date:` header of each email. Emails with missing, malformed, or spoofed `Date:` headers may introduce noise into averages. Extremely large outliers (e.g., a reply timestamped 3 years later due to a malformed header) can significantly skew `slowest_reply_hours`.
- **Null values**: All float fields can be null. Consumers of the API and the UI must handle null gracefully. The `ResponseTimeCard.svelte` component shows "No data" for null fields rather than 0 or NaN.
- **No caching**: Statistics are computed fresh on each request. For contacts with thousands of messages, the query may take noticeable time. No caching layer exists for this endpoint.
- **Fastest/slowest are across both directions**: `fastest_reply_hours` and `slowest_reply_hours` reflect the single fastest and slowest individual reply pair, regardless of direction (their reply or user reply). The card does not separately report fastest/slowest per direction.
- **Email address matching**: Contact identification uses exact string matching on the email address. Different capitalizations of the same address are treated as separate contacts. In practice, email addresses in the `messages` table are normalized to lowercase during sync, but this should be verified if unexpected zero-count results appear.
- **Deleted or re-synced messages**: If messages are deleted from the database (e.g., after a re-sync), historical reply statistics will change. There is no immutable snapshot of statistics.

## Common Questions

**Q: Why are all fields null for a contact I've emailed many times?**
Reply time computation requires at least one reply pair (a message from one party followed by a message from the other). If all messages in the conversation are one-directional — e.g., the user sent emails but the contact never replied, or all fetched messages are from one party — no pairs are identified and all stats are null. Verify the thread contains messages from both parties in the `messages` table.

**Q: What unit are the hours fields in?**
All duration fields (`their_avg_reply_hours`, `your_avg_reply_hours`, `fastest_reply_hours`, `slowest_reply_hours`) are decimal hours. A value of `0.5` means 30 minutes. A value of `24.0` means exactly one day. The UI converts these to human-readable labels (e.g., "30 min", "2h", "1 day") but the API always returns decimal hours.

**Q: Does this feature track whether messages are read before replying?**
No. Reply time is computed from `Date:` headers only — it measures the time between when the previous message was sent and when the reply was sent, not when it was read or opened.

**Q: Why is the "Slow" threshold 24 hours and not something else?**
The thresholds (fast < 1h, medium 1–24h, slow > 24h) are hardcoded in `ResponseTimeCard.svelte`. They are not configurable. This represents typical business-email norms; the thresholds may be revisited if user research suggests different breakpoints.

**Q: Can this be used for all contacts or only contacts I've actually exchanged replies with?**
The endpoint accepts any valid email address. For contacts where no reply pairs exist (one-way communication, or contact not in the database at all), the response returns valid JSON with zero counts and null averages rather than an error.

## Troubleshooting

| Symptom | Likely Cause | Resolution |
|---|---|---|
| Returns 400 | Email in path missing `@` or exceeds 320 chars | Verify URL encoding; check for extra characters in the email path parameter |
| Returns 500 | DB query error | Check `iris-server` logs for the SQL error; verify `messages` table integrity |
| All stats null despite prior email exchanges | No reply pairs found — all messages are consecutive same-sender | Verify the thread in the `messages` table has alternating senders; check date field values for both parties |
| `slowest_reply_hours` is extremely high (thousands of hours) | Malformed or spoofed `Date:` header in one message | Query the `messages` table for the contact and check for outlier `date` values; consider whether the outlier message should be deleted |
| Speed label color wrong | CSS token not resolving in `ResponseTimeCard.svelte` | Check browser console for CSS variable errors; verify `web/src/tokens.css` is loaded |
| Stats change between requests | Messages were added or deleted between requests (no caching) | Expected behavior — stats reflect current DB state on each call |
| Contact popover shows "No data" for response times | Zero reply pairs, or the contact has only sent unsolicited email (no thread with the user) | Normal; advise the user the feature requires bidirectional thread exchanges |

## Related Links

- Backend: `src/api/contacts.rs` (`get_response_times`, `compute_reply_stats`)
- Frontend: `web/src/components/contacts/ResponseTimeCard.svelte`
- Related: FH-018 (Key Topics — shares `/api/contacts/{email}/` namespace and contact popover UI)
- Prior handoffs: FH-003 (Email Reading — message date parsing), FH-005 (Inbox Management — threading model)
