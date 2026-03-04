---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
---

# Tutorial: Using AI Features

This tutorial shows you how to enable Iris's AI features: automatic email classification, thread summarization, AI chat, and writing assist. All AI processing happens locally using Ollama -- no data leaves your machine.

**Time required:** About 10 minutes.

**Prerequisites:**
- Iris is running with at least one email account connected (see [Getting Started with Gmail](tut-001-getting-started-gmail.md))
- You have some emails in your inbox

## Step 1: Install Ollama

If you are using Docker Compose, Ollama is already included as a sidecar service. Skip to Step 2.

For local development, install Ollama:

```bash
# macOS
brew install ollama

# Or download from https://ollama.com
```

Start the Ollama server:

```bash
ollama serve
```

Leave this running in a separate terminal. By default, Ollama listens on `http://localhost:11434`.

## Step 2: Pull an AI Model

You need at least one model for Iris to use. A good starting point is `llama3.2` (3B parameters, fast, reasonable quality):

```bash
ollama pull llama3.2
```

This downloads the model (about 2 GB). Other options:

| Model | Size | Speed | Quality |
|---|---|---|---|
| `llama3.2` | 2 GB | Fast | Good for classification and chat |
| `mistral` | 4 GB | Medium | Good general-purpose model |
| `llama3.1` | 4.7 GB | Medium | Higher quality, more resource-intensive |

## Step 3: Verify the Connection

Open Iris in your browser and navigate to **Settings > AI**.

You should see:

- **Ollama Connection:** A green indicator showing "Connected"
- **Ollama URL:** `http://localhost:11434` (or `http://ollama:11434` in Docker)

If the indicator is red, check that Ollama is running and reachable. Click **Test Connection** to diagnose -- it will show available models if the connection succeeds.

## Step 4: Select a Model and Enable AI

1. In **Settings > AI**, select your model from the **Model** dropdown (e.g., `llama3.2`).
2. Toggle **Enable AI** to on.
3. The settings save automatically.

AI processing is now enabled. Newly synced emails will be classified automatically.

## Step 5: See AI Classifications in Action

Trigger a sync by receiving a new email, or wait for the next IMAP IDLE notification. After classification completes, you will see:

- **Priority badges** next to message subjects in the inbox (e.g., a red badge for "urgent")
- **Category pills** showing the assigned category (e.g., "Primary", "Updates")
- **Category tabs** at the top of the inbox now filter by AI-assigned categories

Open a message to see its full classification: intent, priority, category, and summary.

**Note:** Only newly synced messages are classified. Existing messages in your inbox from before AI was enabled are not retroactively processed.

## Step 6: Try Thread Summarization

1. Open any email thread with multiple messages (a conversation with back-and-forth replies).
2. Look at the top of the thread view for the **Summary** panel.
3. Click to generate a summary if one is not already cached.
4. A concise 2-4 sentence overview appears, covering the key topic, status, and action items.

The summary is cached after the first generation, so opening the same thread again loads it instantly.

## Step 7: Try AI Chat

1. Click the **chat icon** in the sidebar or header to open the chat panel.
2. Type a question like: "What are my most important unread emails?"
3. The AI responds based on your recent emails, with citations linking to specific messages.

More things to try:

- "Summarize the emails from [person] this week"
- "Did anyone send me anything about [topic]?"
- "Archive all the promotional emails" (the AI will propose the action and ask you to confirm)

## Step 8: Try Writing Assist

1. Click **Compose** to open a new email.
2. Write a draft of your message.
3. Click the **AI Assist** dropdown in the compose toolbar.
4. Choose an action:
   - **Rewrite** -- cleans up and professionalizes your text
   - **Formal** -- converts to formal tone
   - **Casual** -- converts to friendly, casual tone
   - **Shorter** -- condenses the text
   - **Longer** -- expands with more detail
5. Review the AI-generated result.

You can apply multiple actions in sequence. For example, write a quick draft, use **Rewrite**, then **Shorter**.

## Step 9: Correct AI Classifications (Optional)

If you notice the AI misclassifying an email:

1. Open the message in the thread view.
2. Click on the classification label you want to correct (category, priority, or intent).
3. Select the correct value.
4. Your correction is saved and used to improve future classifications.

After you have made a few corrections of the same type (at least 2), the AI incorporates your feedback patterns into its classification prompt.

## Troubleshooting

**AI features say "Service Unavailable":**
Check that AI is enabled in **Settings > AI** and that a model is selected. Verify the Ollama connection is green.

**Classifications are not appearing:**
Classifications only apply to newly synced messages. Try sending yourself a test email and waiting for it to sync.

**Summaries are slow:**
Summarization speed depends on your model size and hardware. Smaller models like `llama3.2` are faster. Summaries are cached after the first generation.

**Chat gives unhelpful answers:**
The quality of chat responses depends on the model and the amount of email context available. For better results, use a larger model or ensure the Memories service is running for semantic context retrieval.

## What's Next

- [AI Classification](../features/feat-003-ai-classification.md) -- detailed docs on how classification works
- [AI Chat](../features/feat-005-ai-chat.md) -- everything you can do with the chat assistant
- [AI Feedback](../features/feat-008-ai-feedback.md) -- how corrections improve the system
- [Connecting an Agent](tut-003-connecting-an-agent.md) -- use the Agent API for programmatic access
