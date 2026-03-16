---
status: draft
generated: 2026-03-15
source-tier: direct
hermes-version: 1.0.0
id: feat-014
feature: ai-integration
audience: external
type: feature-doc
---

# Smarter Thread View & AI Suggestions

Iris now surfaces AI insights directly inside your thread view — no need to open a separate panel or run a command. The most useful information appears right where you are reading.

## Thread Intelligence Strip

When you open a thread with multiple messages, a compact strip appears just below the subject line. It shows:

- **Message count** — how many messages are in the thread
- **Action items** — key things that need to be done, extracted from the conversation
- **Deadline** — the earliest date or due date mentioned, if any

The strip is collapsed by default so it does not distract from the email content. Click it to expand the full AI-generated thread summary — a concise 2–4 sentence overview of what the thread is about and where things stand.

This replaces the previous "Summarize" button that required an explicit click and opened a separate summary panel.

## AI Reply Suggestions

For threads where you have not replied and the AI detects a response is expected, a suggestion strip appears at the bottom of the thread. It shows a short preview of a suggested reply.

**To use a suggestion:**

1. Open a thread that shows the AI suggestion strip.
2. Read the preview to see if it fits your intended reply.
3. Click **Reply with this**.
4. The Compose window opens pre-filled with the full suggested text, addressed to the original sender, with the subject pre-filled.
5. Edit the reply as needed, then send — or discard if you prefer to write your own.

The suggestion is a starting point. You always have full control to edit or replace it before sending.

## Contextual AI Chat

When you open AI Chat while reading a specific thread, the assistant automatically loads that thread as context. The chat panel header shows **"Chatting about: [thread subject]"** so you know what context is active.

This means you can ask questions like "What did Alice say about the budget?" or "Draft a follow-up based on this thread" without having to paste in the thread details yourself.

When you open AI Chat from the inbox (not from inside a thread), it works as before — general inbox context with no specific thread loaded.

## Settings Navigation

Switching between tabs in the Settings page now uses a smooth fade crossfade animation instead of an instant cut. The transition is brief (under 200ms) and does not affect keyboard navigation or accessibility.

## Getting the Most Out of AI Suggestions

- **AI classification must be enabled** for the intelligence strip and suggestion strip to populate. Go to **Settings > AI** to confirm your AI provider is connected.
- Suggestions improve over time as you use the AI feedback system to correct misclassifications.
- If a thread does not show the intelligence strip, the AI may not have processed it yet. New messages are classified in the background — check back after a few seconds.
- The "Reply with this" button only appears when the AI has generated a suggestion. If the strip is hidden, no suggestion is available yet for that thread.
