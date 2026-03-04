---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
use-case: ai-chat-with-emails
slug: uc-004-ai-chat-with-emails
---

# Use Case: AI Chat with Emails

## Summary

A user opens the chat panel and asks natural language questions about their email. The system retrieves relevant emails via semantic search, generates an AI response with citations, and can propose inbox actions that the user confirms before execution.

## Actors

- **User**: The person asking questions about their email.
- **System**: The Iris backend (Memories for context retrieval, Ollama for generation).

## Preconditions

- AI is enabled in Settings (ai_enabled = "true") and a model is selected.
- Ollama is running and the selected model is loaded.
- Emails are synced. For best results, Memories is running and emails have been stored for semantic search.

## Flow: Ask a Question

1. User opens the ChatPanel sidebar.
2. User types a question (e.g., "What meetings do I have this week?").
3. Frontend generates or reuses a session_id and calls `POST /api/ai/chat` with `{ session_id, message }`.
4. Backend stores the user message in the chat_messages table.
5. Backend loads the last 10 messages of conversation history for context.
6. Backend performs semantic search via Memories (query: user message, source prefix: `iris/`, limit: 10).
7. For each semantic result, the message ID is extracted and message metadata (subject, from, snippet) is loaded from the database to create citations.
8. If semantic search returns nothing, FTS5 keyword search is used as fallback.
9. Backend builds a prompt with three sections: relevant emails, recent conversation history, and the user's current question.
10. Backend sends the prompt to Ollama with the chat system prompt.
11. Ollama generates a response referencing email citations.
12. Backend parses the response, extracts any ACTION_PROPOSAL suffix, and identifies which citations were referenced.
13. Backend stores the assistant message in chat_messages.
14. Frontend displays the AI response with clickable citation links.

## Flow: Action Proposal and Confirmation

1. User asks "Archive all the LinkedIn notifications from this week."
2. The AI response includes a suggestion to archive specific messages and an ACTION_PROPOSAL JSON.
3. Frontend parses the proposed action and displays a "Confirm" button.
4. User reviews the proposed action (which messages will be affected) and clicks "Confirm."
5. Frontend calls `POST /api/ai/chat/confirm` with `{ session_id, message_id }`.
6. Backend loads the proposed action from the chat message record.
7. Backend executes the batch update (e.g., sets folder = 'Archive' for the specified message IDs).
8. Backend returns `{ executed: true, updated: N }`.
9. Frontend shows a confirmation message (e.g., "Archived 3 messages").

## Flow: Follow-up Questions

1. After the initial response, user asks a follow-up ("What about next week?").
2. The system includes the previous conversation history in the prompt (last 6 messages).
3. The AI can reference context from earlier messages in the same session.

## Postconditions

- User messages and AI responses are stored in the chat_messages table under the session_id.
- If an action was confirmed, the affected messages are updated in the database.
- No emails are sent. Chat actions are limited to inbox management (archive, delete, mark read/unread, star).

## Error Scenarios

| Scenario | System Response |
|---|---|
| AI disabled | 503 Service Unavailable returned |
| Ollama unreachable | 502 Bad Gateway returned |
| No relevant emails found | AI responds honestly that it has no context |
| Model outputs malformed ACTION_PROPOSAL | Proposal is dropped; only the clean response is shown |
| Confirm called with invalid message_id | 404 Not Found |

## Related Features

- fh-009-ai-chat
- fh-011-semantic-memory
- fh-005-inbox-management (batch actions)
