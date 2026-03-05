---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
feature: ai-chat
slug: fh-009-ai-chat
---

# Feature Handoff: AI Chat

## What It Does

AI chat provides a conversational interface where users ask natural language questions about their email. The system retrieves relevant emails as context (RAG), generates AI responses with citations, and can propose inbox actions (archive, delete, star, mark read/unread) that require user confirmation before execution.

## How It Works

### Chat Flow (`src/api/chat.rs`)

`POST /api/ai/chat` accepts `{ session_id, message }`:

1. **Validation**: session_id max 100 chars; message max 50,000 chars.
2. **DB Phase**: Stores the user message in `chat_messages` table. Loads conversation history (last 10 messages). Performs FTS5 search for relevant emails.
3. **Semantic Search Phase**: Calls Memories hybrid search with the user message, source prefix `iris/`, and limit 10.
4. **Citation Resolution**: For semantic results, extracts message IDs from source paths and looks up subject/from/snippet from the messages table. Falls back to FTS5 citations if Memories returns nothing.
5. **Prompt Building**: Constructs a prompt with three sections:
   - `=== Relevant Emails ===` -- citations with truncated IDs, from, subject, snippet
   - `=== Recent Conversation ===` -- last 6 history messages (excluding current)
   - `User: {message}\n\nIris:` -- the current user input
6. **AI Generation**: Sends the prompt to Ollama with the chat system prompt.
7. **Response Parsing**: Extracts clean content and any `ACTION_PROPOSAL:{json}` suffix.
8. **Citation Matching**: Filters citations to those referenced by the AI (by checking if the truncated message ID appears in the response).
9. **Storage**: Stores the assistant message with citations and proposed action JSON in `chat_messages`.

### System Prompt

The chat system prompt instructs the model (named "Iris") to:
- Reference emails by their `[ID]` markers
- Be concise and helpful
- Include `ACTION_PROPOSAL:{json}` for inbox actions
- Valid actions: archive, delete, mark_read, mark_unread, star
- Not fabricate information

### Action Proposals

If the AI response contains `ACTION_PROPOSAL:{"action":"...","description":"...","message_ids":[...]}`:

1. The `parse_action_proposal` function splits the response at the `ACTION_PROPOSAL:` marker.
2. The JSON is parsed into a `ProposedAction` struct.
3. The clean content (before the marker) and the proposed action are stored separately.
4. The frontend displays the action as a confirmable button.

### Action Confirmation

`POST /api/ai/chat/confirm` accepts `{ session_id, message_id }`:

1. Loads the proposed action from the chat message.
2. Maps the action to a SQL UPDATE statement (same batch update pattern as inbox management).
3. Executes the update on the specified message IDs.
4. Returns `{ executed: true, updated: N }`.

### Cross-Session Memory

The chat system loads context from past sessions and user preferences to provide continuity across separate conversations.

**During prompt construction** (`build_chat_prompt`), two additional Memories searches are executed:

1. **Past session summaries**: `memories.search(&input.message, 3, Some("iris/chat/sessions/"))` retrieves the top 3 most relevant past conversation summaries. These are included in the prompt under a `=== Past Conversations ===` section as bullet points.
2. **User preferences**: `memories.search("user email preferences", 1, Some("iris/user/preferences"))` retrieves the user's classification preference profile. This is included under a `=== User Preferences ===` section.

The prompt sections are ordered: User Preferences, Past Conversations, Relevant Emails, Recent Conversation, then the current user message.

If Memories is unreachable or returns no results for either query, the corresponding section is simply omitted from the prompt. The chat continues to function with whatever context is available.

### Auto-Summarization

Chat sessions are automatically summarized to build the cross-session memory. The trigger is message count:

1. After storing an assistant response, the handler counts total messages in the session: `SELECT COUNT(*) FROM chat_messages WHERE session_id = ?`.
2. When `msg_count % 10 == 0` (every 10th message), it calls `enqueue_chat_summarize(&conn, &session_id)`.
3. The `chat_summarize` job (see [fh-013-job-queue](fh-013-job-queue.md)):
   - Loads up to 50 messages from the session.
   - Sends the conversation text to Ollama with the prompt: "Summarize this email assistant conversation in 2-3 sentences, capturing key topics discussed, actions taken, and user preferences revealed."
   - Stores the summary in Memories with source `iris/chat/sessions/{session_id}`.
4. Deduplication: if a `pending` or `processing` `chat_summarize` job for the same session already exists, the enqueue is skipped.

Summaries are overwritten on each summarization (upsert with the session_id as key). A session with 30 messages will be summarized at messages 10, 20, and 30, with each summary replacing the previous one.

### Chat Memory Endpoint

`GET /api/ai/chat/memory` returns the stored cross-session context.

**Response** (`ChatMemoryResponse`):

```json
{
  "summaries": [
    {
      "source": "iris/chat/sessions/abc-123",
      "text": "User asked about quarterly reports from finance team...",
      "score": 0.92
    }
  ],
  "preferences": "- Prefers newsletters categorized as Primary\n- Marks LinkedIn emails as low priority"
}
```

| Field | Description |
|---|---|
| `summaries` | Array of past session summaries from Memories (source prefix `iris/chat/sessions/`), each with source path, text, and similarity score. Up to 10 results. |
| `preferences` | The user's classification preference profile from Memories (`iris/user/preferences`), or `null` if none exists. |

This endpoint queries Memories with a generic search term ("chat conversation summary" for summaries, "user email preferences" for preferences) and is not session-specific.

### Session History

`GET /api/ai/chat/{session_id}` returns all messages for a session (limit 50), ordered by creation time ascending.

## User-Facing Behavior

- The ChatPanel opens as a sliding sidebar from the right edge.
- Users type natural language queries (e.g., "What meetings do I have this week?", "Summarize emails from Alice").
- The AI responds with answers that cite specific emails. Citations are displayed as clickable links.
- If the AI proposes an action, a confirmation button appears. Clicking it executes the action.
- Suggestion chips offer quick queries (e.g., "Morning briefing", "Unread priorities").
- Conversations persist across page navigations within the same session.

## Configuration

Same AI configuration as other features:

| Config Key | Description |
|---|---|
| `ai_enabled` | Must be "true" |
| `ai_model` | Ollama model name |

## Edge Cases and Limitations

- The RAG context is limited to the top 10 search results (semantic or FTS5). The AI cannot access emails outside this context window.
- Action proposals depend on the model correctly formatting the `ACTION_PROPOSAL:` JSON. Smaller models may produce malformed proposals that are silently dropped.
- The citation matching is approximate: it checks if the first 8 characters of the message ID appear in the AI response. This can produce false positives with short or similar IDs.
- Chat sessions are identified by string IDs generated by the frontend. There is no session expiration or cleanup mechanism.
- Cross-session summaries are only generated every 10 messages. Short sessions (under 10 messages) are never summarized and do not contribute to cross-session context.
- Session summaries are overwritten on each summarization cycle. Only the most recent summary for each session is stored in Memories.
- If both Memories and the user have no prior sessions, the chat prompt contains no cross-session context. This is the default state for new installations.
- The conversation history is capped at 10 messages for prompt construction. Older messages are not included in the context, so the AI may lose track of earlier parts of the conversation.
- If both Memories and FTS5 return no results, the AI generates a response without email context. It will typically admit it has no relevant information.
- The confirm action endpoint does not validate that the action was originally proposed for the same session. It trusts the session_id/message_id pair.

## Common Questions

**Q: How does the AI decide which emails to cite?**
A: Emails are retrieved via semantic search (Memories) or keyword search (FTS5). The top 10 results are included as context in the prompt. The AI is instructed to reference these emails using their ID markers.

**Q: What happens if I confirm an action and it fails?**
A: The confirm endpoint returns `{ executed: true, updated: 0 }` if the SQL update matched no rows. This means the message IDs in the proposal were not found or already in the target state.

**Q: Can the AI send emails on my behalf?**
A: No. The chat system only supports inbox management actions (archive, delete, mark read/unread, star). Sending emails requires the compose flow.

## Troubleshooting

| Symptom | Likely Cause | Resolution |
|---|---|---|
| Chat returns 503 | AI not enabled or model not configured | Enable AI in Settings |
| Chat returns 502 | Ollama unreachable | Verify Ollama is running and accessible |
| AI says "I don't have access to your emails" | No search results returned | Check that emails are indexed (FTS5) and stored in Memories |
| Action proposal button missing | Model did not output ACTION_PROPOSAL format | Model-dependent; use a larger model for better instruction following |
| Action confirmation has no effect | Message IDs in proposal don't match actual database IDs | Check that the cited messages exist in the database |
| Chat ignores previous sessions | No summaries stored in Memories | Sessions need 10+ messages to trigger summarization; check Memories health |
| GET /api/ai/chat/memory returns empty | Memories unreachable or no summaries stored | Verify Memories health indicator in Settings; verify sessions reached 10-message threshold |

## Related Links

- Source: `src/api/chat.rs`
- Memories: `src/ai/memories.rs`
- Job queue: `src/jobs/queue.rs` (enqueue_chat_summarize), `src/jobs/worker.rs` (handle_chat_summarize)
- Database: `migrations/002_chat.sql` (chat_messages table)
- Frontend: ChatPanel component
- Related handoff: [fh-013-job-queue](fh-013-job-queue.md)
