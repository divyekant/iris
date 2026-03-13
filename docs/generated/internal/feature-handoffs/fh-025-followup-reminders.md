---
id: fh-025
type: feature-handoff
audience: internal
topic: Follow-up Reminders
status: draft
generated: 2026-03-13
source-tier: direct
context-files: [src/api/followups.rs, web/src/components/FollowupPanel.svelte, migrations/027_followup_reminders.sql]
hermes-version: 1.0.0
---

# Feature Handoff: Follow-up Reminders

## What It Does

Follow-up Reminders identifies sent emails where the user has not received a reply within an expected timeframe and surfaces them as actionable reminders. The AI analyzes each unreplied sent message and assigns an urgency level based on the content — whether the original message contained a deadline, a question, or a direct request. Results are stored in a `followup_reminders` table and displayed in a collapsible `FollowupPanel` component.

Users can manage reminders through three actions: snooze (reschedule for a future date), dismiss (permanently hide), or mark as acted (indicate they have followed up externally). The feature distinguishes between actively pending reminders and snoozed reminders whose snooze period has expired and have become due again.

## How It Works

**Endpoints**:
- `POST /api/ai/scan-followups` — scan sent emails for unreplied threads and create/update reminders
- `GET /api/followups` — list pending reminders and snoozed-but-now-due reminders
- `PUT /api/followups/{id}/snooze` — snooze a reminder to a future date
- `PUT /api/followups/{id}/dismiss` — permanently dismiss a reminder
- `PUT /api/followups/{id}/acted` — mark a reminder as acted upon

---

**POST /api/ai/scan-followups**

Triggers a scan across the user's sent mail to identify unreplied threads. This endpoint is designed to be called periodically (e.g., once per sync cycle via the job queue) or on demand from the UI.

Processing sequence:
1. Queries the `messages` table for all messages where `folder = 'Sent'` and `date` is older than a minimum age threshold (default: 24 hours — messages sent less than 24 hours ago are excluded to avoid false positives on fast-turnaround threads).
2. For each sent message, checks whether the thread contains any subsequent message from a different sender (i.e., a reply). A sent message with at least one reply from another party is excluded.
3. For each unreplied sent message, checks whether a `followup_reminders` record already exists. If it does and is dismissed or acted, it is skipped. If it does not exist, it is a candidate for AI classification.
4. For each new candidate, constructs a prompt providing the sent message's subject and body, asking the AI to assign an urgency level: `low`, `normal`, `high`, or `urgent`. The AI also returns a brief rationale (used internally for debugging; not stored).
5. Stores a row in `followup_reminders` for each new candidate with the AI-assigned urgency.
6. Returns a summary of the scan.

Response:
```json
{
  "scanned": 47,
  "new_reminders": 5,
  "existing_skipped": 12,
  "dismissed_skipped": 3
}
```

**AI urgency classification**:

| Urgency | Criteria (AI-determined) |
|---|---|
| `urgent` | Message contains a deadline that has passed or is imminent, or involves safety/blocking issues |
| `high` | Message contains an explicit request or question that directly blocks work |
| `normal` | Message contains a moderate request or question where a reply is expected |
| `low` | FYI-style message or polite request where a reply is appreciated but not critical |

---

**GET /api/followups**

Returns reminders in two groups:
1. **Pending**: reminders where `status = 'pending'`.
2. **Snoozed and due**: reminders where `status = 'snoozed'` and `snoozed_until <= now()`.

Response:
```json
{
  "pending": [
    {
      "id": 1,
      "message_id": "abc123",
      "thread_id": "thread-xyz",
      "subject": "Q2 Budget Approval Request",
      "sent_at": "2026-03-08T14:00:00Z",
      "urgency": "high",
      "status": "pending",
      "days_waiting": 5,
      "created_at": "2026-03-09T02:00:00Z"
    }
  ],
  "due_snoozed": []
}
```

`days_waiting` is computed at query time as the number of calendar days between `sent_at` and now.

---

**PUT /api/followups/{id}/snooze**

Request body:
```json
{
  "snooze_until": "2026-03-20T09:00:00Z"
}
```

Sets `status = 'snoozed'` and `snoozed_until` to the provided ISO 8601 datetime. Returns 400 if `snooze_until` is in the past. Returns 404 if the reminder ID does not exist. Snoozed reminders are excluded from GET /api/followups until `snoozed_until` has elapsed.

---

**PUT /api/followups/{id}/dismiss**

Sets `status = 'dismissed'`. Dismissed reminders are permanently excluded from GET /api/followups. The record remains in the database for auditability but is never returned by the list endpoint. Idempotent.

---

**PUT /api/followups/{id}/acted**

Sets `status = 'acted'`. Indicates the user has followed up through some channel external to Iris (e.g., a phone call, Slack message). Acts like dismiss for display purposes but carries semantic distinction in the data. Idempotent.

---

**Migration 027 — `followup_reminders` table**:

| Column | Type | Notes |
|---|---|---|
| `id` | INTEGER PRIMARY KEY | Auto-increment |
| `message_id` | TEXT NOT NULL UNIQUE | FK to messages.id — one reminder per sent message |
| `thread_id` | TEXT NOT NULL | Thread the message belongs to |
| `subject` | TEXT | Cached message subject |
| `sent_at` | TEXT NOT NULL | ISO 8601 send timestamp from messages.date |
| `urgency` | TEXT NOT NULL | `low`, `normal`, `high`, or `urgent` |
| `status` | TEXT NOT NULL DEFAULT 'pending' | `pending`, `snoozed`, `dismissed`, `acted` |
| `snoozed_until` | TEXT | ISO 8601 datetime; null unless status = snoozed |
| `created_at` | TEXT NOT NULL | Row creation timestamp |

`message_id` has a UNIQUE constraint — each sent message produces at most one reminder row, regardless of how many times `scan-followups` is called.

**Implementation files**:
- `src/api/followups.rs` — all route handlers, scan logic, AI prompt construction
- `migrations/027_followup_reminders.sql` — schema migration
- `web/src/components/FollowupPanel.svelte` — collapsible sidebar/panel component

## User-Facing Behavior

**FollowupPanel** is a collapsible panel in the inbox layout, positioned in the sidebar or below the inbox list. It renders:
- A count badge showing the number of pending + due-snoozed reminders (using the brand gold unread indicator token, consistent with the unread badge design).
- A grouped list: pending reminders first, then due-snoozed reminders.
- Each row shows: subject, recipient, days waiting, urgency badge.

**Urgency badges** use the priority badge color conventions from the project design standards:
- `urgent` → `--iris-color-error`
- `high` → `--iris-color-warning`
- `normal` → `--iris-color-success`
- `low` → `--iris-color-text-faint`

**Row actions**: Each reminder row has three inline actions:
- **Snooze**: Opens a date-time picker; submits PUT /api/followups/{id}/snooze.
- **Dismiss**: Calls PUT /api/followups/{id}/dismiss; removes row immediately.
- **Acted**: Calls PUT /api/followups/{id}/acted; removes row immediately.

**Scan trigger**: The panel includes a "Scan for follow-ups" button that calls POST /api/ai/scan-followups and refreshes the panel on completion. Automatic scanning on sync is not included in the initial implementation.

**Clicking a reminder row**: Opens the thread in the thread view at the original sent message, allowing the user to compose a follow-up or review the context.

**Empty state**: When no reminders are pending or due, the panel shows "No follow-ups needed" with an empty state illustration. If the scan has never been run, the panel prompts the user to run their first scan.

## Configuration

No user-configurable settings specific to this feature. The minimum message age threshold (24 hours) is hardcoded in `src/api/followups.rs`.

| Parameter | Value | Location |
|---|---|---|
| Minimum sent message age | 24 hours | `src/api/followups.rs` — `FOLLOWUP_MIN_AGE_HOURS` constant |
| AI required for scan | Yes — urgency classification uses ProviderPool |
| GET endpoint AI dependency | None — reads from `followup_reminders` table only |
| Unique constraint | `message_id` — one reminder per sent message |

## Edge Cases & Limitations

- **No automatic scanning on sync**: Reminders are only created when POST /api/ai/scan-followups is called explicitly. Messages received after the last scan are not automatically evaluated. Periodic scanning via the job queue is the recommended integration path.
- **AI required only for scan**: The GET, snooze, dismiss, and acted endpoints all read from or update the `followup_reminders` table without AI calls. If the AI provider is unavailable, existing reminders remain accessible and manageable.
- **One reminder per sent message**: The `UNIQUE` constraint on `message_id` means that re-scanning cannot create duplicate reminders. If a reminder exists (in any status), re-scanning skips that message. A dismissed reminder is not re-created even if weeks pass without a reply.
- **Urgency does not update on re-scan**: If a reminder was created with urgency `low` and the context has changed (e.g., the deadline is now imminent), the urgency is not automatically updated. Re-evaluation would require deleting the existing reminder and re-running the scan.
- **Thread reply detection is message-based**: The scan checks for any message in the thread from a different sender. If a thread has been replied to externally (e.g., the user received a reply that was not synced, or the reply is in a folder not synced), the sent message will still appear as unreplied. Users should run sync before scanning to get current thread state.
- **Snoozed-until in the past triggers on next GET**: Snoozed reminders with `snoozed_until` elapsed are returned by GET /api/followups in the `due_snoozed` group. There is no background job that automatically re-surfaces them — they surface on the next time the client calls GET.
- **`days_waiting` is display-only**: The `days_waiting` value is computed at query time and not stored. It reflects the current date at request time.
- **Sent folder assumption**: The scan filters on `folder = 'Sent'`. If sent messages are stored in a differently-named folder (e.g., "Sent Items" for some Outlook accounts), they may not be detected. Folder naming normalization should be verified for non-Gmail accounts.

## Common Questions

**Q: Will a reminder be created for a message I sent to myself?**
Self-sent messages (from and to the same address) will appear as unreplied if no other message exists in the thread from a different sender. Whether this is desirable depends on use case. The current implementation does not explicitly exclude self-sent messages. A filter could be added to exclude messages where all recipients are the user's own account addresses.

**Q: What if I send a follow-up email and then the original reminder becomes irrelevant?**
The scan does not automatically detect that the user sent a follow-up in a new thread. If the user composed a follow-up email in the original thread, that follow-up will itself be scanned as a new sent message potentially needing a reply. The original reminder should be manually marked as "acted" via the PUT endpoint or dismissed. Future enhancement could detect sent-follow-up events and auto-dismiss related reminders.

**Q: How is urgency different from AI Classification (FH-007) priority?**
AI Classification (FH-007) assigns a priority to incoming messages based on their content. Follow-up urgency assesses sent messages based on whether they contained time-sensitive or blocking requests. The two systems are independent. A message classified as `normal` priority by FH-007 when received could generate a `high` urgency follow-up reminder if it contained an unreplied request.

**Q: Can I see all dismissed reminders for audit purposes?**
The GET /api/followups endpoint does not return dismissed or acted reminders. The records remain in the `followup_reminders` table and can be queried directly if needed, but no UI is provided for reviewing dismissed reminders.

**Q: What happens if the same thread gets multiple sent messages without replies?**
Each sent message is evaluated independently. If the user sent three follow-up messages in the same thread and received no reply, three separate reminder rows can exist (one per `message_id`). All three would appear in the panel. Users would typically dismiss or snooze the earlier ones and leave the most recent active.

## Troubleshooting

| Symptom | Likely Cause | Resolution |
|---|---|---|
| POST /api/ai/scan-followups returns 503 | No AI provider healthy | Verify provider configuration in Settings; fix provider before scanning |
| Scan returns `new_reminders: 0` despite known unreplied sent mail | Minimum age threshold not met (messages sent < 24 hours ago); or messages are in wrong folder name | Verify sent messages are older than 24 hours; check `folder` column values in `messages` table for sent items |
| GET /api/followups returns empty after scan | Scan ran but found no candidates meeting criteria | Confirm sent messages exist with no replies; check scan response `scanned` count |
| Known unreplied message not appearing | Reminder already exists with status `dismissed` or `acted` | Check `followup_reminders` table for `message_id`; dismissed entries are not re-created by scan |
| PUT /api/followups/{id}/snooze returns 400 | `snooze_until` date is in the past | Provide a future datetime |
| Snoozed reminders not reappearing when due | Client has not called GET /api/followups since the snooze expired | Re-fetch — due-snoozed reminders appear in `due_snoozed` group on next GET |
| Urgency seems wrong for a message | AI misclassified based on content | Mark as acted and dismiss; urgency is not re-evaluated after creation |
| FollowupPanel shows wrong count | Panel count badge not refreshing after action | Ensure PUT actions trigger a GET /api/followups refresh in `FollowupPanel.svelte` |

## Related Links

- Backend: `src/api/followups.rs`
- Migration: `migrations/027_followup_reminders.sql`
- Frontend: `web/src/components/FollowupPanel.svelte`
- Related: FH-007 (AI Classification — priority signals complement urgency), FH-013 (Job Queue — periodic scan-followups integration), FH-021 (Deadline Extraction — separate but related time-sensitivity feature), FH-017 (Task Extraction — action-item detection from received mail, counterpart to follow-up scanning of sent mail)
- Design tokens: `web/src/tokens.css` (urgency badge colors via `--iris-color-error`, `--iris-color-warning`, `--iris-color-success`, `--iris-color-text-faint`)
