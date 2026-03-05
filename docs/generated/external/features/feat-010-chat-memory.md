---
id: feat-010
title: Cross-Session Chat Memory
audience: external
generated: 2026-03-04
status: current
---

# Cross-Session Chat Memory

The Iris AI chat assistant now remembers past conversations. When you start a new chat session, the assistant has context from your previous discussions, so you do not have to repeat yourself or re-explain your preferences.

## How It Works

As you use the chat assistant, Iris automatically summarizes your conversations in the background. Every 10 messages in a session, a summary is generated and stored. When you open a new chat session, the assistant receives relevant summaries from past sessions as context, allowing it to pick up where you left off.

For example, if you asked the assistant last week to help you find all emails about a project deadline, it can reference that discussion in future conversations without you needing to re-explain the context.

## What to Expect

- **Continuity across sessions.** The assistant may reference relevant past discussions when answering your questions.
- **Preference awareness.** If you have expressed preferences in past chats (e.g., "I prefer concise summaries"), the assistant carries those forward.
- **Gradual improvement.** The more you use the chat assistant, the more context it accumulates and the more helpful its responses become.

The assistant does not recall every word from every conversation. It works from summaries, so the context it carries forward captures the key points and patterns rather than exact phrasing.

## Viewing Stored Memory

You can see what the chat assistant remembers by querying the memory endpoint:

```
GET /api/ai/chat/memory
```

**Example request:**

```bash
curl -s -H "X-Session-Token: your_session_token" \
  "http://localhost:3000/api/ai/chat/memory"
```

**Response:**

```json
{
  "summaries": [
    {
      "session_id": "sess-abc-123",
      "summary": "User asked about emails from the marketing team regarding the Q1 campaign. Found 12 relevant threads and drafted a follow-up.",
      "created_at": "2026-03-02T14:30:00Z"
    },
    {
      "session_id": "sess-def-456",
      "summary": "User prefers concise summaries. Discussed project deadline emails and identified three action items.",
      "created_at": "2026-03-03T09:15:00Z"
    }
  ],
  "preferences": [
    "Prefers concise email summaries over detailed ones",
    "Wants action items highlighted in thread summaries"
  ]
}
```

| Field | Description |
|---|---|
| `summaries` | Past session summaries the assistant can reference |
| `preferences` | Patterns and preferences extracted from your conversations |

## Privacy

All chat memory stays entirely local. Summaries and preferences are stored in your Memories instance running on your machine. No conversation data is sent to external services. If you want to clear the assistant's memory, you can do so through the Memories service directly.
