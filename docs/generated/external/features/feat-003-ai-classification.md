---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
---

# AI Classification

Iris automatically classifies incoming emails using a local AI model. Each message receives an intent label, priority level, category, and extracted entities. This happens in the background as emails arrive, so your inbox is organized without any manual effort.

## How It Works

When a new email is synced from your mail server, Iris sends it to your local Ollama instance for classification. The AI model analyzes the subject, sender, and body text, then returns structured metadata:

| Field | Values | Description |
|---|---|---|
| **Intent** | ACTION_REQUEST, INFORMATIONAL, TRANSACTIONAL, SOCIAL, MARKETING, NOTIFICATION | What kind of email this is |
| **Priority** | urgent, high, normal, low | How time-sensitive the message is |
| **Category** | Primary, Updates, Social, Promotions, Finance, Travel, Newsletters | Inbox category for filtering |
| **Summary** | Free text | A one-sentence summary of the email content |

Classification runs as a background task after each sync cycle, so it does not slow down message delivery.

## What You See

In the inbox, classified messages display:

- **Priority badges** -- color-coded labels (e.g., red for urgent, orange for high) next to the message subject
- **Category pills** -- small labels showing the assigned category
- **Category tabs** -- the inbox header tabs (Primary, Updates, Social, Promotions) filter messages by their AI-assigned category

In the thread view, you can see the full classification details for each message.

## Setup

### Prerequisites

- **Ollama** must be installed and running. Iris connects to it at the URL specified in `OLLAMA_URL` (default: `http://localhost:11434`).
- You need at least one AI model pulled in Ollama (e.g., `ollama pull llama3.2`).

### Configuration

1. Go to **Settings > AI**.
2. Verify the **Ollama Connection** indicator shows green (connected).
3. Click **Test Connection** to see available models.
4. Select a model from the dropdown (e.g., `llama3.2`).
5. Toggle **Enable AI** to on.

Once enabled, all newly synced messages will be classified automatically. Existing messages are not retroactively classified.

### Choosing a Model

Any model available in your Ollama instance can be used. Smaller models (e.g., `llama3.2` at 3B parameters) are faster and use less memory. Larger models may produce more accurate classifications but require more resources.

## Limitations

- **Requires Ollama running.** If Ollama is not reachable, classification is silently skipped. Messages are still synced and readable; they just lack AI metadata.
- **No retroactive classification.** Only messages synced after AI is enabled are classified. Existing messages in your inbox are not processed.
- **Model quality varies.** Classification accuracy depends on the model you choose and the nature of your emails.
- **Local processing only.** All classification happens on your machine. No email content is sent to external services.

## Improving Accuracy

If you notice the AI misclassifying emails, you can correct it. See [AI Feedback](feat-008-ai-feedback.md) for details on how to submit corrections that improve future classifications.
