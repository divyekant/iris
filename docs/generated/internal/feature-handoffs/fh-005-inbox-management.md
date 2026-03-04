---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
feature: inbox-management
slug: fh-005-inbox-management
---

# Feature Handoff: Inbox Management

## What It Does

Inbox management provides batch actions on messages, category-based filtering, account switching, and view mode toggling. It enables users to organize their inbox efficiently through bulk operations and filtered views.

## How It Works

### Batch Actions (`src/api/messages.rs`)

`PATCH /api/messages/batch` accepts a JSON body with `ids` (array of message IDs, max 1000) and `action` (string). Supported actions:

| Action | SQL Effect |
|---|---|
| `archive` | Sets `folder = 'Archive'` |
| `delete` | Sets `folder = 'Trash'` |
| `mark_read` | Sets `is_read = 1` |
| `mark_unread` | Sets `is_read = 0` |
| `star` | Sets `is_starred = 1` |
| `unstar` | Sets `is_starred = 0` |

The batch size is capped at 1000 to prevent unbounded SQL IN clauses. All updates set `updated_at = unixepoch()`. The response returns the count of rows updated.

### Category Tabs

Messages are filtered by category using the `labels` JSON column and `ai_category` column. The `GET /api/messages` endpoint accepts a `category` query parameter which generates a LIKE clause on the `labels` column: `labels LIKE '%"{category}%'`. The category value is escaped for LIKE wildcards (`%`, `_`) to prevent SQL injection.

Available categories (set by AI classification): Primary, Updates, Social, Promotions, Finance, Travel, Newsletters.

### Message Listing

`GET /api/messages` supports these query parameters:

- `account_id` -- filter to single account (omit for unified inbox)
- `folder` -- defaults to "INBOX"
- `category` -- filter by AI-assigned category
- `limit` -- max 500, default 50
- `offset` -- pagination offset

The response includes:
- `messages` -- array of `MessageSummary` objects
- `unread_count` -- count of unread messages matching the filter
- `total` -- total count matching the filter

For unified inbox (no account_id specified), messages are joined with the `accounts` table filtering by `is_active = 1`.

### Account Switcher

The account switcher in the frontend passes `account_id` to the messages API. When no account is selected, the unified inbox shows messages from all active accounts merged by date.

### View Mode Toggle

View mode (compact/comfortable) is stored in the `config` table via `GET/PUT /api/config/view-mode`. The frontend reads this on load and adjusts the message list density accordingly.

## User-Facing Behavior

- Checkbox selection on messages enables a bulk action toolbar with Archive, Delete, Mark Read/Unread, and Star buttons.
- Category tabs (Primary, Updates, Social, Promotions) filter the inbox view. The active tab is highlighted.
- The account switcher dropdown shows all connected accounts with their email addresses.
- View mode toggle switches between compact (more messages visible) and comfortable (more spacing) layouts.

## Configuration

View mode and theme preferences are persisted in the `config` table:

- `theme` -- "system", "light", or "dark"
- `view_mode` -- "compact" or "comfortable"

## Edge Cases and Limitations

- Batch actions are local-only. Archive/delete/star operations are not synced back to the IMAP server.
- The category filter uses a LIKE query on the `labels` JSON column, which is a text-based approximation. It works for the current JSON format but is not a proper JSON query.
- Batch update with more than 1000 IDs is rejected with 400 Bad Request.
- Invalid action strings are rejected with 400 Bad Request.
- Soft delete (moving to Trash) does not permanently remove messages. There is no trash-emptying mechanism.
- The unified inbox includes messages from all active accounts. Deactivated accounts are excluded from the join.

## Common Questions

**Q: Are batch actions synced to the email provider (Gmail, Outlook)?**
A: No. All batch actions modify only the local SQLite database. The remote IMAP server is not updated. This means archiving or deleting locally does not archive or delete on the provider.

**Q: How are categories assigned?**
A: Categories are assigned by the AI classification pipeline during email sync (V6). The `ai_category` field is set to one of the predefined categories. Users can correct categories via the AI feedback mechanism.

**Q: What is the maximum number of messages I can act on at once?**
A: 1000 messages per batch request. This limit prevents excessive SQL IN clause sizes.

## Troubleshooting

| Symptom | Likely Cause | Resolution |
|---|---|---|
| Batch action returns 400 | Empty ids array, too many ids (>1000), or invalid action | Validate the request body |
| Category tab shows no messages | AI classification not run or no messages match | Ensure AI is enabled and messages have been classified |
| Unified inbox missing messages from an account | Account is deactivated (is_active = 0) | Reactivate the account |
| Unread count seems wrong after batch mark_read | Count query uses the same filter as listing | Refresh the page to get updated counts |

## Related Links

- Source: `src/api/messages.rs` (list_messages, batch_update_messages), `src/api/config.rs`
- Models: `src/models/message.rs` (batch_update, MessageSummary)
- Frontend: Inbox page, CategoryTabs, AccountSwitcher, ViewModeToggle components
