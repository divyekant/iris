---
id: feat-021
type: feature-doc
audience: external
topic: bulk-chat-ops
status: draft
generated: 2026-03-13
source-tier: direct
hermes-version: 1.0.0
---

# Bulk Chat Operations

## Overview

Bulk Chat Operations lets you manage large groups of emails by describing what you want in plain language inside the AI Chat panel. Instead of selecting emails one by one and clicking through menus, you type something like "archive all emails from LinkedIn older than 30 days" and Iris identifies the matching messages, shows you what it found, and waits for your confirmation before making any changes.

Every bulk operation requires an explicit confirmation step. Iris will never modify your inbox without your approval.

---

## Getting Started

1. Open the AI Chat panel (the chat icon in the top navigation bar).
2. Type a natural language request describing the batch action you want. Be as specific or broad as you like.
3. Iris will respond with a list of matching emails and a summary of the operation it plans to perform.
4. Confirm by typing "yes" or "confirm" (or decline by typing "cancel") to proceed.

No API calls are needed — this feature is driven entirely through the chat interface.

---

## Supported Actions

| Action | What it does |
|---|---|
| `archive` | Moves messages out of the inbox, keeps them searchable |
| `mark_read` | Marks matched messages as read |
| `mark_unread` | Marks matched messages as unread |
| `trash` | Moves messages to trash |
| `star` | Adds a star to matched messages |
| `unstar` | Removes the star from matched messages |
| `move_to_category` | Reassigns messages to a category (e.g., Newsletters, Work) |

---

## Chat Examples

### Archive emails from a specific sender

> You: Archive all emails from LinkedIn.

Iris responds:

> Found 47 emails from linkedin.com domains (jobs@linkedin.com, notifications@linkedin.com, messages-noreply@linkedin.com). I'll archive all 47. Confirm?

Type **confirm** to proceed, or **cancel** to abort.

---

### Clean up a newsletter category

> You: Mark all newsletters as read.

Iris responds:

> Found 112 unread emails in the Newsletters category. I'll mark all 112 as read. Confirm?

---

### Move promotional emails to a category

> You: Move all emails from Shopify and Amazon to the Newsletters category.

Iris responds:

> Found 23 emails from shopify.com and 31 emails from amazon.com — 54 total. I'll move them to the Newsletters category. Confirm?

---

### Trash old unread emails

> You: Trash unread emails older than 90 days that aren't starred.

Iris responds:

> Found 89 unread, unstarred emails older than 90 days. I'll send them to trash. This cannot be undone automatically — confirm?

---

## API Reference

Bulk Chat Operations is driven through the existing chat endpoint. There is no separate bulk API — Iris handles intent detection and action proposal internally.

```
POST /api/ai/chat
```

**Example — triggering a bulk operation via API:**

```bash
curl -X POST http://localhost:3000/api/ai/chat \
  -H "x-session-token: $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "message": "Archive all emails from newsletter@example.com",
    "session_id": "chat_session_001"
  }'
```

**Response (action proposal):**

```json
{
  "reply": "Found 18 emails from newsletter@example.com. I'll archive all 18. Confirm?",
  "action_proposal": {
    "action": "archive",
    "matched_message_ids": ["msg_1", "msg_2", "msg_3"],
    "total_matched": 18,
    "requires_confirmation": true
  }
}
```

**Confirming via API:**

```bash
curl -X POST http://localhost:3000/api/ai/chat \
  -H "x-session-token: $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "message": "confirm",
    "session_id": "chat_session_001"
  }'
```

**Response (action executed):**

```json
{
  "reply": "Done. Archived 18 emails from newsletter@example.com.",
  "action_result": {
    "action": "archive",
    "executed": true,
    "affected_count": 18
  }
}
```

---

## Safety and Confirmation

Every bulk operation proposal has `"requires_confirmation": true`. Iris will never execute a destructive or large-scale change without waiting for your explicit confirmation in the same chat session.

If you start a new chat session or close the panel before confirming, the proposal is discarded. No partial changes are made.

For `trash` operations specifically, Iris will note that the action cannot be easily undone, giving you an extra moment to reconsider.

---

## FAQ

**Can I undo a bulk operation after confirming?**
Currently there is no one-click undo. For `archive` and `move_to_category` operations you can reverse the action with another chat request (e.g., "move all archived emails from LinkedIn back to inbox"). For `trash` operations, email clients typically retain items in trash for 30 days before permanent deletion.

**What if I describe something ambiguous, like "delete old emails"?**
Iris will ask clarifying questions before showing a proposal. It will not guess at what "old" means — it will ask you to specify a time range or other criteria. The confirmation step only appears once the target set of messages is unambiguous.

**Is there a limit to how many emails I can operate on at once?**
There is no hard cap, but Iris will flag operations affecting more than 500 emails and ask you to confirm twice. This is a safety guard, not a restriction.

---

## Related

- [AI Chat (feat-005)](feat-005-ai-chat.md)
- [Email Management (feat-001)](feat-001-email-management.md)
- [AI Classification (feat-003)](feat-003-ai-classification.md)
