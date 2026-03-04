---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
---

# AI Chat

Iris includes a conversational AI assistant that can answer questions about your emails, summarize your inbox, and propose actions -- all through natural language.

## How to Use

1. Click the **chat icon** in the sidebar or header to open the chat panel.
2. Type a question or request in natural language.
3. The AI responds with an answer, drawing on your recent emails for context.
4. Continue the conversation -- the AI remembers the context within your session.

## What You Can Ask

Here are some examples of what the AI chat can help with:

| Request | Example |
|---|---|
| **Inbox briefing** | "What are my most important unread emails?" |
| **Find information** | "Did anyone email me about the Q4 budget?" |
| **Summarize** | "Summarize the emails from Alice this week" |
| **Action proposals** | "Archive all the LinkedIn notifications" |
| **Lookup** | "What was the flight confirmation number in my travel email?" |

## Citations

When the AI references specific emails in its response, it includes **citations** -- short references that link back to the original message. Citations show the sender name, subject, and a snippet so you can verify the information.

## Action Proposals

If you ask the AI to perform an action (like archiving, deleting, or marking messages as read), it does not execute immediately. Instead, it proposes the action and asks for your confirmation:

1. The AI describes what it would do (e.g., "Archive 3 emails from LinkedIn").
2. A **Confirm** button appears in the chat.
3. Click **Confirm** to execute the action, or ignore it to skip.

Supported actions:

| Action | Description |
|---|---|
| `archive` | Move messages to the Archive folder |
| `delete` | Move messages to the Trash folder |
| `mark_read` | Mark messages as read |
| `mark_unread` | Mark messages as unread |
| `star` | Star messages |

## Session Persistence

Chat conversations are stored in the database and persist across page reloads. Each conversation has a session ID, and you can return to a previous session to continue where you left off. Chat history is loaded automatically when you reopen the chat panel.

## How Context Works

The AI builds context from two sources:

1. **Semantic search** (if Memories is available) -- the AI searches your email archive by meaning to find the most relevant messages to your question.
2. **Keyword search** (FTS5 fallback) -- if semantic search is unavailable or returns no results, the AI uses keyword matching to find relevant emails.

The AI receives up to 10 relevant email snippets as context, along with the recent conversation history (up to 6 previous messages).

## Suggestion Chips

Below the chat input, you may see **suggestion chips** -- quick-tap prompts for common questions like "Brief me on today's emails" or "What needs my attention?" Click a chip to send it as your message.

## Prerequisites

- **Ollama** running and reachable, with a model selected in **Settings > AI**
- AI processing enabled in Settings
- For best results, the **Memories** semantic search service should be running (improves context retrieval)

If Ollama is not available, the chat endpoint returns a service unavailable error.
