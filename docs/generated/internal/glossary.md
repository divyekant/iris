---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
---

# Iris Glossary

## API Key

A secret token (`iris_` + 32 hex characters) used to authenticate external agents with the Agent API. Only the SHA-256 hash is stored in the database. Keys have one of four permission levels (read_only, draft_only, send_with_approval, autonomous) and can optionally be scoped to a single account.

## Cross-Session Memory

The system by which context from past chat sessions is carried forward into new conversations. When a chat session reaches 10 messages, a `chat_summarize` job generates a 2-3 sentence summary and stores it in Memories at `iris/chat/sessions/{session_id}`. On new chat sessions, the top 3 most relevant past summaries are loaded from Memories and included in the AI prompt. User classification preferences are also loaded. The stored context can be inspected via `GET /api/ai/chat/memory`.

## DKIM (DomainKeys Identified Mail)

An email authentication mechanism that allows the receiving server to verify that an email was authorized by the domain owner. The sender's server signs the email with a private key; the recipient verifies the signature using a public key published in DNS. Iris parses DKIM results from the Authentication-Results header and displays them as trust indicators.

## DMARC (Domain-based Message Authentication, Reporting, and Conformance)

A policy layer built on top of SPF and DKIM that tells receiving servers how to handle emails that fail authentication. Policies include none (monitor), quarantine, and reject. Iris parses DMARC results from the Authentication-Results header.

## DOMPurify

A JavaScript library used in the Iris frontend to sanitize HTML email content before rendering. It strips scripts, event handlers, and other dangerous HTML elements. Iris also strips inline style attributes to prevent CSS-based attacks. The sanitized HTML is rendered inside a sandboxed iframe for additional isolation.

## Exponential Backoff

A retry strategy where the delay between attempts increases exponentially. In the Iris job queue, the backoff formula is `attempts^2 * 5` seconds: first retry after 5s, second after 20s, third after 45s. After the 4th attempt (configurable via `max_attempts`), the job is permanently marked as `failed`. This prevents overwhelming external services (Ollama, Memories) when they are temporarily unavailable while still retrying transient failures.

## Entity Extraction

The process by which the AI classification pipeline identifies structured data within an email: people's names, dates and deadlines, monetary amounts, and key topics or project names. Extracted entities are stored as a JSON object in the `ai_entities` column.

## FTS5 (Full-Text Search 5)

SQLite's built-in full-text search engine. Iris uses FTS5 with porter stemming and unicode61 tokenization to index email subjects, body text, sender names, and sender addresses. The FTS5 `fts_messages` virtual table is kept in sync with the `messages` table via database triggers.

## IDLE

An IMAP extension (RFC 2177) that allows a client to maintain a persistent connection to the server and receive real-time push notifications when new messages arrive or flags change. Iris uses IDLE with a 29-minute timeout, after which it disconnects, re-syncs, and re-enters IDLE mode.

## Job Queue

A SQLite-backed persistent work queue (`processing_jobs` table) that manages asynchronous processing tasks. Replaces fire-and-forget `tokio::spawn` calls with durable, retryable jobs. Supports four job types: `ai_classify`, `memories_store`, `chat_summarize`, and `pref_extract`. Jobs have statuses (`pending`, `processing`, `done`, `failed`), retry counts, and exponential backoff. Queue health is exposed via `GET /api/ai/queue-status`. Source: `src/jobs/queue.rs`.

## Job Worker

The background Tokio task that polls the `processing_jobs` table and executes jobs. Runs as a single long-lived loop spawned at server startup. Polls at a configurable interval (`job_poll_interval_ms`, default 2000ms), claims batches of up to 10 pending jobs, and processes them concurrently up to a semaphore limit (`job_max_concurrency`, default 4). Periodically cleans up completed jobs older than `job_cleanup_days` (default 7). Source: `src/jobs/worker.rs`.

## Intent Classification

The AI pipeline's categorization of an email's purpose into one of six types: ACTION_REQUEST (requires the user to do something), INFORMATIONAL (FYI), TRANSACTIONAL (receipts, confirmations), SOCIAL (personal correspondence), MARKETING (promotional), NOTIFICATION (system alerts).

## Memories MCP

A vector store server that provides semantic search capabilities. Iris stores email content as vector embeddings in Memories during sync, enabling natural language queries that match by meaning rather than exact keywords. The client communicates via HTTP at the configured `MEMORIES_URL` (default: `http://localhost:8900`).

## MIME (Multipurpose Internet Mail Extensions)

The standard for structuring email messages. MIME allows emails to contain multiple parts (text, HTML, attachments) organized in a tree structure. Iris uses the `mailparse` Rust crate to parse MIME-structured emails during sync, extracting text/plain, text/html, and attachment metadata.

## Processing Job

A single unit of asynchronous work tracked in the `processing_jobs` SQLite table. Each row represents a job with a type (`ai_classify`, `memories_store`, `chat_summarize`, `pref_extract`), an optional linked `message_id`, a status (`pending`, `processing`, `done`, `failed`), attempt count, max attempts (default 4), JSON payload, error message, and timestamps for creation, update, and next retry. Jobs are enqueued by sync, chat, and feedback handlers, and processed by the Job Worker.

## Ollama

A local LLM inference server that runs open-source language models. Iris uses Ollama for all AI features: email classification, thread summarization, writing assist, and conversational chat. The server runs at the configured `OLLAMA_URL` (default: `http://localhost:11434`).

## Preference Extraction

The process by which user classification preferences are synthesized from accumulated AI feedback corrections. Every 10 user corrections (via `PUT /api/messages/{id}/ai-feedback`), a `pref_extract` job is enqueued. The job loads the top 20 correction patterns from the `ai_feedback` table, sends them to Ollama to generate a 3-5 bullet point preference profile, and stores the result in Memories at `iris/user/preferences`. This profile is then loaded and injected into future AI classification prompts, allowing the model to adapt to the user's preferences beyond individual correction patterns.

## Priority Score

A floating-point value between 0.0 and 1.0 assigned by the AI classification pipeline to indicate email urgency. 0.0 is least urgent; 1.0 is most urgent. The score is also mapped to a human-readable label: urgent (0.8-1.0), high (0.6-0.8), normal (0.3-0.6), low (0.0-0.3). These thresholds are approximate and model-dependent.

## RAG (Retrieval-Augmented Generation)

A technique where relevant documents are retrieved from a data store and provided as context to a language model before generating a response. Iris uses RAG in the AI chat feature: emails are retrieved via semantic search (Memories) or keyword search (FTS5) and included in the Ollama prompt so the model can answer questions about the user's email.

## Semantic Search

Search that understands the meaning of a query rather than matching exact keywords. Iris provides semantic search through the Memories vector store, which uses embedding models to represent both queries and email content as vectors in a high-dimensional space. Similar meanings produce similar vectors, enabling matching by concept rather than word.

## Session Token

A 64-character random hex string generated when the Iris server starts. The web UI retrieves this token via the same-origin `/api/auth/bootstrap` endpoint and includes it in the `X-Session-Token` header on all protected API requests. The token changes on every server restart.

## SPF (Sender Policy Framework)

An email authentication mechanism that allows domain owners to specify which mail servers are authorized to send email on behalf of their domain. The receiving server checks the sender's IP against the domain's SPF DNS record. Iris parses SPF results from the Authentication-Results header.

## Thread ID

An identifier that groups related email messages into a conversation. Iris derives the thread ID from email headers: it uses the first message-id in the `References` header (the thread root), or the `In-Reply-To` header, or falls back to the message's own `Message-ID`. All angle brackets are stripped.

## Tracking Pixel

A tiny (typically 1x1 pixel) invisible image embedded in HTML emails to track whether the recipient opened the email. When the email is rendered, the image is loaded from the tracker's server, revealing the recipient's IP address and open time. Iris detects tracking pixels by checking for tiny images and known tracker domains (Mailchimp, SendGrid, HubSpot, etc.).

## XOAUTH2

A SASL authentication mechanism that allows IMAP and SMTP clients to authenticate using OAuth2 access tokens instead of passwords. The mechanism sends a base64-encoded string containing the user email and Bearer token. Iris uses XOAUTH2 for both IMAP (via async-imap) and SMTP (via lettre) when connecting to Gmail and Outlook.
