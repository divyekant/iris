---
status: current
generated: 2026-03-26
source-tier: direct
hermes-version: 1.0.1
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

## Agent Platform (Unified Auth)

**Q: What changed from the old agent API to the unified auth?**
A: The old agent API used separate `/api/agent/*` routes with their own auth middleware. The unified auth middleware now runs on all 200+ protected routes and accepts both session tokens (browser) and Bearer API keys (agents). The old routes still work for backward compatibility, but new integrations should use Bearer auth on the standard API routes.

**Q: Can an agent use all the same endpoints as the web UI?**
A: Yes, subject to permission checks. An agent with `autonomous` permission can access every endpoint the web UI uses. Lower permission levels restrict access -- for example, `read_only` cannot create drafts or send emails.

**Q: How does the permission hierarchy work?**
A: Four levels ordered from least to most privileged: `read_only` < `draft_only` < `send_with_approval` < `autonomous`. A higher permission always satisfies a lower one. For example, a `send_with_approval` key can read, draft, and send, but cannot use `execute` or `configure` actions reserved for `autonomous`.

**Q: Can I scope an API key to a single email account?**
A: Yes. When creating an API key, set the optional `account_id` field. The key can then only access messages from and send via that account. Attempts to access other accounts return 403.

**Q: How do reply and forward endpoints handle threading?**
A: `POST /api/reply` and `POST /api/forward` resolve the original message, build proper `In-Reply-To` and `References` headers from stored raw headers, construct the reply/forward body with quoting, and handle recipient deduplication for reply-all. Draft variants (`POST /api/drafts/reply`, `POST /api/drafts/forward`) create drafts without sending.

**Q: What rate limits apply to agent requests?**
A: Each API key gets its own rate limit bucket (keyed by `agent:{key_prefix}`). The general limit is 500 burst with ~5/sec sustained rate. Auth endpoints have a stricter limit of 10 burst at 1/sec. When exceeded, HTTP 429 is returned with `retry-after` headers.

**Q: How do MCP tool permissions work with API keys?**
A: Each MCP tool maps to a minimum permission level. Read-only tools (search, read, list) require `read_only`. Draft and action tools (create_draft, archive, star) require `draft_only`. Send and chat tools require `send_with_approval`. Permission denials return MCP-formatted errors (HTTP 200 with `status: "permission_denied"`), not HTTP 403.

**Q: Can an agent create other API keys?**
A: Only if the agent's key has `autonomous` permission. Creating and revoking keys requires the highest permission level. Lower-permission agents cannot manage keys.

## Memories & Semantic Search (v5)

**Q: What is `document_at` and why does it matter?**
A: `document_at` is an ISO 8601 timestamp set on each email memory entry, representing the email's original send date. It enables temporal filtering via `since` and `until` query parameters. Without it, entries cannot participate in date-range searches.

**Q: Why is recency weight set to zero for email search?**
A: Standard Memories search penalizes older entries, but email relevance is topic-based, not time-based. A three-year-old email about a contract is as relevant to a "contract" search as one from yesterday. Zero recency weight treats all entries equally, while `since/until` parameters provide explicit temporal filtering when needed.

**Q: What is graph_weight and when does it help?**
A: Graph weight (set to 0.1 for email search) gives a small ranking boost to results connected by entity relationships in the knowledge graph. For example, if emails about "Project Alpha" mention "Acme Corp," searching for "Acme Corp" may boost "Project Alpha" results slightly. The low weight ensures text relevance still dominates.

## Showcase Features

**Q: How do delegation playbooks decide when to act?**
A: Each playbook defines trigger conditions (sender domain, subject contains, AI category, intent) and a confidence threshold (default 0.85). All specified conditions must match (AND logic). Conditions set to null are ignored. The delegation engine only acts when match confidence meets or exceeds the threshold.

**Q: Can I undo a delegation action?**
A: Yes. Call `POST /api/delegation/actions/{id}/undo`. The action record is updated and the original state is restored where possible (e.g., un-archiving an auto-archived message).

**Q: How does auto-draft use my writing style?**
A: When generating a draft, the system loads stored style traits (greeting, signoff, tone, formality, vocabulary) and includes them in the AI prompt. The AI uses these to match your natural writing voice. Run `POST /api/style/{account_id}/analyze` to populate traits before using auto-draft.

**Q: What entity types does the knowledge graph extract?**
A: Five types: `person`, `org` (company/organization), `project` (product/initiative), `date`, `amount`. Entity resolution is case-insensitive but type-sensitive -- "Acme" as `org` and "Acme" as `project` are treated as separate entities.

**Q: How does temporal search resolve natural-language queries?**
A: The temporal reasoning engine sends the query along with known timeline events and today's date to the AI. The AI resolves it to a concrete date range (e.g., "around the board meeting" becomes 2026-02-15 to 2026-03-01). If no temporal reference is detected, it defaults to the last 30 days.

## Production & Deployment

**Q: Why is the version hardcoded in the health endpoint?**
A: The `env!("CARGO_PKG_VERSION")` macro captures the version at compile time. With Docker layer caching, if `Cargo.toml` is unchanged between builds, the cached layer retains the old version string. Hardcoding in `src/api/health.rs` ensures correctness at the cost of manual updates.

**Q: Does the Docker container run as root?**
A: No. The runtime stage creates a non-root user `iris` (UID 1001) and all processes run under that user. Volume mounts for `/app/data` must be writable by UID 1001.

**Q: What security headers does Iris set?**
A: Four headers on every response: `x-content-type-options: nosniff`, `x-frame-options: DENY`, `referrer-policy: strict-origin-when-cross-origin`, and `permissions-policy: camera=(), microphone=(), geolocation=()`. Content-Security-Policy is not set because the SPA requires inline styles/scripts.

**Q: How do I configure CORS for production?**
A: Set the `IRIS_CORS_ORIGINS` environment variable to a comma-separated list of allowed origins (e.g., `https://mail.example.com`). If unset, the system falls back to localhost development origins and logs a warning.
