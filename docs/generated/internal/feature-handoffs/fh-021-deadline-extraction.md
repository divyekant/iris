---
id: fh-021
type: feature-handoff
audience: internal
topic: Deadline Extraction
status: draft
generated: 2026-03-13
source-tier: direct
context-files: [src/api/deadlines.rs, web/src/components/DeadlineList.svelte, web/src/components/DeadlineWidget.svelte, migrations/025_deadlines.sql]
hermes-version: 1.0.0
---

# Feature Handoff: Deadline Extraction

## What It Does

Deadline Extraction uses AI to identify time-sensitive obligations mentioned in email content and stores them as structured deadline records. When a user triggers extraction on a message, the AI scans the body for deadline signals — explicit dates, relative phrases like "by Friday" or "end of next week", and implicit urgency markers — and returns one or more deadline entries with resolved absolute dates. Extracted deadlines are persisted in a dedicated `deadlines` table and surfaced through list and widget components in the UI.

The feature covers the full lifecycle of a deadline: extraction, display, completion, and deletion. Users can mark deadlines complete or remove them. Deadlines scoped to a specific thread are retrievable without filtering through all deadlines globally.

## How It Works

**Endpoints**:
- `POST /api/ai/extract-deadlines` — run AI extraction on a message
- `GET /api/deadlines` — list deadlines across all messages
- `GET /api/threads/{id}/deadlines` — list deadlines scoped to a thread
- `PUT /api/deadlines/{id}/complete` — mark a deadline complete
- `DELETE /api/deadlines/{id}` — delete a deadline

---

**POST /api/ai/extract-deadlines**

Request body:
```json
{
  "message_id": "abc123"
}
```

Processing sequence:
1. Validates `message_id` is present and non-empty.
2. Fetches message body, subject, and the message's `date` header from the `messages` table. The message `date` is passed to the AI as the reference point for resolving relative date expressions.
3. Constructs a prompt that provides the message content plus the message send date, instructing the AI to return a JSON array of deadline objects. Each object must include `description` (string), `due_date` (ISO 8601 date string), and `confidence` (0.0–1.0).
4. Dispatches through `ProviderPool`. Parses the JSON array from the AI response.
5. For each returned deadline:
   - Validates `due_date` parses as a valid date via `chrono`.
   - If `due_date` is relative (some models return relative strings despite the prompt), the handler attempts to resolve against the message's send date using `chrono::Local`.
   - Inserts a row into `deadlines`. The `(message_id, description)` pair is unique-constrained — if the same deadline description is extracted again for the same message, the existing record is updated (`ON CONFLICT DO UPDATE`) rather than duplicated.
6. Returns the list of extracted (or updated) deadline records.

Response:
```json
{
  "message_id": "abc123",
  "deadlines": [
    {
      "id": 1,
      "message_id": "abc123",
      "description": "Submit Q1 budget report",
      "due_date": "2026-03-21",
      "completed": false,
      "confidence": 0.88,
      "created_at": "2026-03-13T10:00:00Z"
    }
  ]
}
```

**Relative date resolution**: The AI is given the message's `date` header as context ("Today's date when this email was sent: YYYY-MM-DD"). Phrases like "by Friday" are resolved by the AI into absolute ISO dates. The Rust handler validates the resulting date with `chrono`; if parsing fails, the deadline entry is skipped and logged at WARN.

---

**GET /api/deadlines**

Query parameters:

| Parameter | Type | Default | Description |
|---|---|---|---|
| `days` | integer | 7 | Return deadlines due within the next N days from today |
| `include_completed` | boolean | false | If true, include deadlines where `completed = true` |

Returns deadlines ordered by `due_date` ascending. The `days` filter applies a `due_date <= today + N days` constraint. There is no lower bound — overdue deadlines (due dates in the past) are included unless `include_completed=false` excludes already-completed ones.

---

**GET /api/threads/{id}/deadlines**

Returns all deadlines associated with messages belonging to the specified thread. No date range filter — returns all deadlines for the thread regardless of due date or completion status. The caller is responsible for client-side filtering if needed.

---

**PUT /api/deadlines/{id}/complete**

Marks a deadline as completed by setting `completed = true` and `completed_at` to the current timestamp. Idempotent — calling again on an already-completed deadline has no effect and returns 200. Returns 404 if the deadline ID does not exist.

---

**DELETE /api/deadlines/{id}**

Permanently deletes the deadline record. Returns 404 if not found. No soft-delete; deletion is irreversible.

---

**Migration 025 — `deadlines` table**:

| Column | Type | Notes |
|---|---|---|
| `id` | INTEGER PRIMARY KEY | Auto-increment |
| `message_id` | TEXT NOT NULL | FK to messages.id |
| `description` | TEXT NOT NULL | Extracted deadline description |
| `due_date` | TEXT NOT NULL | ISO 8601 date (YYYY-MM-DD) |
| `completed` | INTEGER NOT NULL DEFAULT 0 | Boolean (0/1) |
| `completed_at` | TEXT | ISO 8601 timestamp, null until completed |
| `confidence` | REAL NOT NULL | AI-reported confidence (0.0–1.0) |
| `created_at` | TEXT NOT NULL | Insert timestamp |

Unique constraint on `(message_id, description)`.

**Implementation files**:
- `src/api/deadlines.rs` — all route handlers and AI prompt logic
- `migrations/025_deadlines.sql` — schema migration
- `web/src/components/DeadlineList.svelte` — full deadline list with filters
- `web/src/components/DeadlineWidget.svelte` — compact upcoming deadlines widget

## User-Facing Behavior

**DeadlineList** renders a sortable, filterable list of all deadlines. It supports toggling between "upcoming" (default, within 7 days) and "all" views, and allows toggling completed deadlines in/out of the list. Each row shows description, due date, message source link, and a "Mark complete" action. Overdue deadlines render with a visual indicator (red text or icon) to distinguish them from upcoming ones.

**DeadlineWidget** is a compact component suitable for embedding in the inbox sidebar or thread view. It shows the next N deadlines (typically 3–5) ordered by due date ascending. It links to the full DeadlineList for overflow.

**Thread-scoped deadlines** appear within the thread view when a thread contains messages with extracted deadlines. The widget renders inline below the thread header.

**Extraction trigger**: Extraction is initiated by the user via a button in the thread view ("Extract deadlines from this message") or through a thread-level action. There is no automatic extraction on sync.

**Completion flow**: Clicking "Mark complete" calls PUT /api/deadlines/{id}/complete and updates the row in-place (no full list reload). The row either fades out (if `include_completed=false`) or displays a strikethrough with a checkmark.

## Configuration

| Consideration | Detail |
|---|---|
| AI required | Yes — POST /api/ai/extract-deadlines requires a healthy provider |
| Reference date | Message send date (`date` column in `messages`) used as relative-date anchor |
| Date format | AI expected to return ISO 8601 (YYYY-MM-DD); handler validates and rejects malformed dates |
| Unique constraint | `(message_id, description)` — re-extraction updates rather than duplicates |
| No configurable settings | Deadline extraction has no user-configurable parameters |

## Edge Cases & Limitations

- **No automatic extraction on sync**: Deadlines are not extracted automatically when email arrives. A user must trigger extraction explicitly. A job queue integration for auto-extraction is not included in this implementation.
- **Relative date resolution depends on message date**: If the message `date` header is absent, malformed, or far in the past, relative expressions ("by Friday") may resolve to incorrect absolute dates. The handler uses the message's stored `date` as the reference; it does not use the current date at extraction time.
- **AI may return zero deadlines**: If the AI finds no deadline signals in the message content, it returns an empty array. This is a valid response and not an error. The endpoint returns an empty `deadlines` array.
- **Multi-deadline messages**: A single message can produce multiple deadline rows. Each is stored separately with its own `(message_id, description)` key.
- **Duplicate description collision**: The unique constraint on `(message_id, description)` uses exact string matching. Minor variations in AI-returned descriptions for the same deadline (e.g., "Submit report by Friday" vs. "Submit report") will create separate rows rather than updating the same one.
- **Past deadlines not auto-cleaned**: Overdue deadlines remain in the table indefinitely unless the user deletes them. The `GET /api/deadlines?days=7` filter does not exclude past-due deadlines; it only sets an upper bound.
- **Confidence is AI-reported**: As with intent detection, confidence reflects the model's self-assessment. A high-confidence deadline from a poorly-calibrated model can still be incorrect.
- **No snooze support**: Unlike FollowupReminders (FH-025), deadlines do not support snoozing. The only lifecycle states are active and completed (or deleted).
- **Thread deadline endpoint returns all deadlines**: `GET /api/threads/{id}/deadlines` returns all deadlines for that thread regardless of date or completion. Filtering logic lives on the client.

## Common Questions

**Q: What if the AI extracts a deadline that does not actually exist in the email?**
False positives are possible. Confidence scores can help identify uncertain extractions (low confidence values indicate the AI was not sure), but there is no automatic filtering. Users should review extracted deadlines before relying on them. The DELETE endpoint allows removing false positives.

**Q: Why does re-extraction update existing records instead of creating new ones?**
The `ON CONFLICT DO UPDATE` behavior on `(message_id, description)` prevents the same deadline from accumulating duplicate rows if a user extracts from the same message multiple times. It also means the `due_date` and `confidence` values are refreshed if the AI returns a different result on re-extraction.

**Q: Is the `due_date` stored as a date or a datetime?**
Stored as a date string (YYYY-MM-DD) without a time component. If the email specifies a specific time ("by 5pm Friday"), the time is dropped — only the date is stored. Time-of-day precision is not supported in the current schema.

**Q: Does the thread deadlines endpoint also scan messages that haven't been extracted?**
No. `GET /api/threads/{id}/deadlines` queries the `deadlines` table, which only contains records that were explicitly extracted via the POST endpoint. If extraction was not triggered for a message in the thread, no deadlines for that message appear.

**Q: How does deadline extraction interact with Task Extraction (FH-017)?**
These are separate features with separate storage. Task Extraction (FH-017) identifies action items from email content and stores them in the `tasks` table. Deadline Extraction identifies time-bounded obligations and stores them in the `deadlines` table. A task may have an associated deadline, but the two tables are not linked in the schema.

## Troubleshooting

| Symptom | Likely Cause | Resolution |
|---|---|---|
| POST returns 503 | No AI provider healthy | Check Settings AI configuration; verify provider reachability |
| POST returns 404 | `message_id` not found | Confirm message has synced; verify correct message ID |
| POST returns empty `deadlines` array | AI found no deadline signals in message content | Expected for messages without dates or time-bound obligations; no action needed |
| `due_date` value is incorrect (wrong year/date) | AI misresolved a relative date expression | Check the message's `date` column value; if malformed, the reference date used by AI may be wrong |
| Same deadline appears twice | Description strings differ slightly between extractions | Expected behavior; `ON CONFLICT` key is exact-string match on description |
| Overdue deadlines not disappearing | They are not auto-deleted | User must mark complete or delete manually; deadlines persist until acted on |
| PUT /complete returns 404 | Deadline ID does not exist | Verify ID from GET response; check if deadline was already deleted |
| DeadlineWidget shows no entries | No deadlines due within the default 7-day window | Check if extraction has been run; or deadlines exist but due dates are outside the window |

## Related Links

- Backend: `src/api/deadlines.rs`
- Migration: `migrations/025_deadlines.sql`
- Frontend: `web/src/components/DeadlineList.svelte`, `web/src/components/DeadlineWidget.svelte`
- Related: FH-017 (Task Extraction — similar AI extraction pattern, separate storage), FH-013 (Job Queue — auto-extraction integration path), FH-007 (AI Classification — shared ProviderPool dispatch)
