---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
---

# Iris FAQ

## Accounts

**Q: Which email providers does Iris support?**
A: Iris supports Gmail and Outlook via OAuth2 (automatic IMAP/SMTP configuration), and any provider that supports IMAP/SMTP via manual configuration with app passwords.

**Q: Can I connect multiple email accounts?**
A: Yes. Each account must have a unique email address. The unified inbox shows messages from all active accounts merged by date. You can also filter by account using the account switcher.

**Q: What happens if my OAuth token expires?**
A: Iris automatically refreshes OAuth tokens when they are within 60 seconds of expiration. The refresh uses the stored refresh token to obtain a new access token. If the refresh token itself is revoked (e.g., user revokes app access in Google settings), you need to re-authenticate by reconnecting the account.

**Q: How do I disconnect an account?**
A: Delete the account via `DELETE /api/accounts/{id}`. This removes the account record and its stored credentials. Note: messages already synced remain in the local database. IMAP IDLE listeners for the account will stop.

## Email Sync

**Q: How many emails does Iris sync?**
A: Initial sync fetches the newest 100 messages from the INBOX folder. Subsequent new messages are picked up in real time via IMAP IDLE push notifications.

**Q: Does Iris sync all folders?**
A: No. Currently only the INBOX folder is synced and monitored via IDLE. Sent, Drafts, Spam, and custom folders are not synced.

**Q: Are local changes (archive, delete, star) synced back to the provider?**
A: No. All inbox management actions (archive, delete, mark read/unread, star) only modify the local SQLite database. The remote IMAP server is not updated. This is a known limitation.

**Q: What happens during a network outage?**
A: The IDLE listener will detect the connection drop and retry with exponential backoff (starting at 30 seconds, capping at 15 minutes). Once connectivity is restored, it reconnects and re-syncs.

## AI Features

**Q: What AI model should I use?**
A: Instruction-tuned models with good JSON output work best for classification. Recommended: Llama 3.2 (8B), Mistral 7B, Gemma 2 9B. Smaller models (1-3B) may produce inconsistent JSON. For summarization and chat, larger models generally produce better results.

**Q: Does Iris send my email data to external services?**
A: No. All AI processing uses a local Ollama instance running on your machine. Email data never leaves your device. The Memories vector store also runs locally.

**Q: Can I disable AI features entirely?**
A: Yes. Set `ai_enabled` to false in Settings. All AI features (classification, summarization, chat, writing assist) will be disabled. Email sync and other core features continue to work.

**Q: How does the AI feedback loop work?**
A: When you correct an AI classification (category, priority, or intent), the correction is stored. When the same correction pattern occurs 2 or more times, it is appended to the classification prompt as a hint, so the model adjusts future classifications.

**Q: Why is AI classification slow during initial sync?**
A: Classification runs via the job queue with a concurrency limit of 4 tasks (configurable via `job_max_concurrency`). During initial sync of 100 messages, 200 jobs are enqueued (one `ai_classify` + one `memories_store` per message). Each AI classification takes 1-5 seconds depending on model size and hardware. All 100 messages may take several minutes to classify. Monitor progress via `GET /api/ai/queue-status`.

**Q: What happens if Ollama is down during email sync?**
A: Email sync completes normally -- messages are inserted into the database. AI classification and Memories storage jobs are enqueued into the `processing_jobs` table. When the job worker picks them up and Ollama is unreachable, the jobs fail and are retried with exponential backoff (5s, 20s, 45s). If Ollama remains down through all 4 attempts, the jobs are permanently marked as `failed`. Once Ollama recovers, it only processes newly enqueued jobs. Failed jobs can be manually retried by resetting their status in SQLite.

**Q: How do I check job queue health?**
A: Call `GET /api/ai/queue-status`. It returns `{ pending, processing, failed, done_today }`. Key things to watch: `failed > 0` indicates jobs that exhausted retries (check Ollama/Memories connectivity). `pending` growing without `done_today` increasing means the worker may not be running. `processing` stuck at a non-zero value for extended periods suggests stuck jobs.

**Q: How does cross-session chat memory work?**
A: Every 10 messages in a chat session, a `chat_summarize` job is enqueued. The job loads the session's messages, sends them to Ollama for summarization (2-3 sentences), and stores the summary in Memories at `iris/chat/sessions/{session_id}`. On subsequent chat sessions, the prompt builder searches Memories for the top 3 most relevant past session summaries and includes them in the prompt under "Past Conversations." User preferences (extracted from AI feedback corrections) are also loaded and included under "User Preferences." View stored memory via `GET /api/ai/chat/memory`.

## Search

**Q: What is the difference between keyword and semantic search?**
A: Keyword search (FTS5) finds emails containing the exact words you typed (with stemming). Semantic search (Memories) understands meaning and can find related content even when different words are used. For example, searching "financial concerns" via semantic search can match emails about "budget issues."

**Q: How do I use search filters?**
A: Use the filter chips below the search bar: toggle "has:attachment" to filter emails with attachments, use date range pickers to filter by time period, or select a specific account. These filters combine with the text query.

**Q: Why does semantic search sometimes return the same results as keyword search?**
A: If the Memories server is unreachable or returns no results, semantic search silently falls back to FTS5 keyword search. Check the Memories health indicator in Settings.

## Agent API

**Q: How do I authenticate an external agent?**
A: Create an API key in Settings > API Keys. The key is displayed once at creation. The agent includes it in requests as `Authorization: Bearer iris_{key}`.

**Q: What permission levels are available?**
A: Four levels: `read_only` (search and read), `draft_only` (adds draft creation), `send_with_approval` (adds email sending), `autonomous` (adds execute and configure, reserved for future use).

**Q: Can I restrict an agent to a single account?**
A: Yes. When creating an API key, optionally specify an account_id. The agent can then only access messages from and send via that account.

**Q: How do I audit what an agent has done?**
A: Navigate to Settings > Audit Log. All agent actions are logged with timestamps, the key used, action type, resource accessed, and success/failure status.

## Security

**Q: How does authentication work?**
A: Iris generates a random 64-character session token on each startup. The web UI retrieves this token via a same-origin-protected bootstrap endpoint. All subsequent API calls include the token in the `X-Session-Token` header. A server restart generates a new token.

**Q: Is HTTPS required?**
A: For local-only use (127.0.0.1), HTTP is acceptable since traffic does not leave the machine. For any network-accessible deployment, HTTPS should be used to protect the session token and API keys in transit.

**Q: What are trust indicators?**
A: Trust indicators show the SPF, DKIM, and DMARC verification results for received emails. These are parsed from the `Authentication-Results` header. Pass means the email's origin is verified; Fail means it may be spoofed.

**Q: What is tracking pixel detection?**
A: Iris scans HTML emails for 1x1 pixel images and images from known email tracking services (Mailchimp, SendGrid, HubSpot, etc.). Detected tracking pixels are flagged in the message view. They are not blocked.

**Q: How are API keys stored?**
A: API keys are hashed with SHA-256 before storage. Only the hash is in the database. The raw key is shown once at creation and cannot be recovered.
