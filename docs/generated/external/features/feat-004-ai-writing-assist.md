---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
---

# AI Writing Assist and Thread Summarization

Iris provides two AI-powered writing tools: **thread summarization** to quickly understand long email conversations, and **writing assist** to help you compose better emails.

## Thread Summarization

When you open a thread with multiple messages, Iris can generate a concise summary of the entire conversation.

### How to Use

1. Open any email thread from the inbox.
2. Look for the **Summary** panel at the top of the thread view.
3. If a summary is not yet available, click to generate one.
4. The summary appears as a 2-4 sentence overview covering the key topic, current status, and any action items.

### Details

- **Cached.** Once generated, summaries are stored and reused. Subsequent views of the same thread load the cached summary instantly.
- **Concise.** Summaries are 2-4 sentences, covering the main topic, the current status or outcome, and any action items or next steps.
- **Works on any thread length.** For very long threads, the AI focuses on the most recent and relevant messages.

## Writing Assist

The writing assist feature helps you refine email text using AI. It is available in the compose modal when writing a new email, replying, or forwarding.

### How to Use

1. Open the compose modal (new email, reply, reply all, or forward).
2. Write your draft text.
3. Click the **AI Assist** dropdown in the compose toolbar.
4. Choose an action:

| Action | What it does |
|---|---|
| **Rewrite** | Rewrites your text to be clearer and more professional |
| **Formal** | Converts to a formal, professional tone |
| **Casual** | Converts to a casual, friendly tone |
| **Shorter** | Condenses the text while keeping key points |
| **Longer** | Expands the text with more detail |

5. The AI-generated result replaces or supplements your current text in the compose area.

### Tips

- You can apply multiple actions in sequence. For example, write a quick draft, use **Rewrite** to clean it up, then use **Shorter** to trim it down.
- The AI preserves the meaning of your original text. It changes the style, not the substance.
- Writing assist works on any amount of text, from a single sentence to a full email.

## Prerequisites

Both features require:

- **Ollama** running and reachable
- An AI model selected in **Settings > AI**
- AI processing enabled in Settings

If Ollama is not available, the summary panel and writing assist dropdown are still visible but return an error when used.
