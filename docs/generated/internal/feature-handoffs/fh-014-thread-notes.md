---
id: fh-014
type: feature-handoff
audience: internal
topic: Thread Notes
status: draft
generated: 2026-03-13
source-tier: direct
hermes-version: 1.0.1
---

# FH-014: Thread Notes

## What It Does

Thread Notes lets users attach private, personal annotations to any email thread. Notes are stored locally in Iris and are never transmitted — they are not visible to senders, recipients, or any external party. A user can create multiple notes per thread, edit them, or delete them at any time.

Primary use cases: tracking follow-up reminders, adding context from phone calls or meetings related to a thread, documenting decisions made outside email.

## How It Works

**Storage**: Notes are persisted in the `thread_notes` SQLite table (migration `023_thread_notes.sql`). Each row holds:

| Column | Type | Notes |
|---|---|---|
| `id` | TEXT | 16-character lowercase hex string, generated at creation |
| `thread_id` | TEXT | Must match a valid thread in the `threads` table |
| `content` | TEXT | The note body; 1–10,000 characters |
| `created_at` | TEXT | ISO-8601 UTC timestamp |
| `updated_at` | TEXT | ISO-8601 UTC timestamp; updated on PUT |

**Implementation files**:
- `src/api/thread_notes.rs` — all four route handlers (list, create, update, delete)
- `web/src/components/thread/NotesPanel.svelte` — collapsible panel in ThreadView

**Ordering**: GET returns notes sorted by `created_at DESC` (newest first).

## User-Facing Behavior

- In ThreadView, a **Notes** panel sits below the email body. It is collapsed by default and toggles open when the user clicks it.
- Each note is displayed as a gold-tinted card showing the note text and its creation timestamp.
- Users type a new note into a text area inside the panel and press **Save**. Empty submissions are blocked at the UI level before the request is sent.
- Existing notes show an **Edit** and **Delete** control. Edit enters inline editing mode; the Save action sends a PUT. Delete sends a DELETE and removes the card immediately.
- There is no character counter in the current UI; the 10,000-character limit is enforced server-side.

## Configuration

No feature flags or configuration required. Thread Notes is always available once migration `023_thread_notes.sql` has run. The feature shares the standard session authentication used by all Iris API routes.

## API Reference

| Method | Path | Body | Success |
|---|---|---|---|
| GET | `/api/threads/{thread_id}/notes` | — | 200 `{"notes": [...]}` |
| POST | `/api/threads/{thread_id}/notes` | `{"content": "..."}` | 201 `{"id": "...", "content": "...", "created_at": "...", "updated_at": "..."}` |
| PUT | `/api/threads/{thread_id}/notes/{id}` | `{"content": "..."}` | 200 updated note object |
| DELETE | `/api/threads/{thread_id}/notes/{id}` | — | 200 `{"deleted": true}` |

**Error responses**:

| Status | Cause |
|---|---|
| 400 | Content is empty or whitespace-only |
| 404 | `thread_id` does not exist in `threads` table |
| 404 | Note `id` not found (on PUT/DELETE) |
| 422 | Content exceeds 10,000 characters |

## Edge Cases & Limitations

- **Thread deletion**: If a thread is purged from the database (e.g., during a re-sync that removes orphaned threads), its notes are not automatically deleted. The notes remain in `thread_notes` but are orphaned. This is low-risk because thread IDs are stable and threads are rarely hard-deleted.
- **No account scoping**: Notes are stored by `thread_id` only. If two accounts share the same thread ID (extremely unlikely given ID derivation from Message-ID headers), they would share notes. In practice this does not occur.
- **No search**: Notes content is not indexed in FTS5. Users cannot search notes text through the standard search interface.
- **No export**: Notes are not included in any export or backup mechanism in the current release.
- **16-character hex IDs**: IDs are randomly generated at insert time using a cryptographic random source. They are not sequential and carry no ordering information.

## Common Questions

**Q: Are notes synced to Gmail or Outlook?**
No. Notes are stored only in the local Iris SQLite database (`iris.db`). They are never written back to the email provider and are not visible in Gmail, Outlook, or any other client. If the database is deleted or the application is reinstalled, notes are lost.

**Q: Can another Iris user see my notes?**
No, unless they have direct access to the same `iris.db` file. Iris is a single-user local application. Notes have no sharing or collaboration mechanism.

**Q: Is there a per-thread note limit?**
No. The API and schema impose no limit on the number of notes per thread. Users can create as many notes as needed; only the per-note 10,000-character limit applies.

**Q: What happens if I edit a note and submit the same content?**
The PUT endpoint accepts the request and returns 200. The `updated_at` timestamp is refreshed even if content is unchanged. No conflict detection or dirty-check is performed.

**Q: Can I search my notes?**
Not in the current release. Notes are stored in `thread_notes.content` but are not added to the FTS5 virtual table. A future iteration could index notes content if user demand warrants it.

## Troubleshooting

| Symptom | Likely Cause | Resolution |
|---|---|---|
| POST returns 422 | Content exceeds 10,000 characters or is empty | Reduce note length; ensure content field is non-empty |
| POST/GET returns 404 | `thread_id` in the URL does not exist | Verify the thread is loaded in the inbox; the thread may have been deleted or re-synced with a new ID |
| PUT returns 404 | Note ID does not match any record for that thread | Refresh the UI; the note may have been deleted in another session |
| Notes panel not visible | ThreadView did not load the panel component | Check browser console for Svelte rendering errors; confirm `NotesPanel.svelte` is bundled |
| Notes disappeared after re-sync | Database reset or migration re-run wiped `thread_notes` | Notes are not recoverable unless a DB backup exists; advise user to back up `iris.db` |
| `updated_at` not changing | Client is sending PUT but hitting a cached response | Confirm `X-Session-Token` header is present; caching should not apply to mutation routes |

## Related Links

- Migration: `migrations/023_thread_notes.sql`
- Backend handler: `src/api/thread_notes.rs`
- Frontend panel: `web/src/components/thread/NotesPanel.svelte`
- Prior handoffs: FH-003 (email reading / ThreadView), FH-009 (AI Chat — similar panel pattern)
