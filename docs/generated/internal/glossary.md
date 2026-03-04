---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
---

# Iris Glossary

## API Key

A secret token (`iris_` + 32 hex characters) used to authenticate external agents with the Agent API. Only the SHA-256 hash is stored in the database. Keys have one of four permission levels (read_only, draft_only, send_with_approval, autonomous) and can optionally be scoped to a single account.

## DKIM (DomainKeys Identified Mail)

An email authentication mechanism that allows the receiving server to verify that an email was authorized by the domain owner. The sender's server signs the email with a private key; the recipient verifies the signature using a public key published in DNS. Iris parses DKIM results from the Authentication-Results header and displays them as trust indicators.

## DMARC (Domain-based Message Authentication, Reporting, and Conformance)

A policy layer built on top of SPF and DKIM that tells receiving servers how to handle emails that fail authentication. Policies include none (monitor), quarantine, and reject. Iris parses DMARC results from the Authentication-Results header.

## DOMPurify

A JavaScript library used in the Iris frontend to sanitize HTML email content before rendering. It strips scripts, event handlers, and other dangerous HTML elements. Iris also strips inline style attributes to prevent CSS-based attacks. The sanitized HTML is rendered inside a sandboxed iframe for additional isolation.

## Entity Extraction

The process by which the AI classification pipeline identifies structured data within an email: people's names, dates and deadlines, monetary amounts, and key topics or project names. Extracted entities are stored as a JSON object in the `ai_entities` column.

## FTS5 (Full-Text Search 5)

SQLite's built-in full-text search engine. Iris uses FTS5 with porter stemming and unicode61 tokenization to index email subjects, body text, sender names, and sender addresses. The FTS5 `fts_messages` virtual table is kept in sync with the `messages` table via database triggers.

## IDLE

An IMAP extension (RFC 2177) that allows a client to maintain a persistent connection to the server and receive real-time push notifications when new messages arrive or flags change. Iris uses IDLE with a 29-minute timeout, after which it disconnects, re-syncs, and re-enters IDLE mode.

## Intent Classification

The AI pipeline's categorization of an email's purpose into one of six types: ACTION_REQUEST (requires the user to do something), INFORMATIONAL (FYI), TRANSACTIONAL (receipts, confirmations), SOCIAL (personal correspondence), MARKETING (promotional), NOTIFICATION (system alerts).

## Memories MCP

A vector store server that provides semantic search capabilities. Iris stores email content as vector embeddings in Memories during sync, enabling natural language queries that match by meaning rather than exact keywords. The client communicates via HTTP at the configured `MEMORIES_URL` (default: `http://localhost:8900`).

## MIME (Multipurpose Internet Mail Extensions)

The standard for structuring email messages. MIME allows emails to contain multiple parts (text, HTML, attachments) organized in a tree structure. Iris uses the `mailparse` Rust crate to parse MIME-structured emails during sync, extracting text/plain, text/html, and attachment metadata.

## Ollama

A local LLM inference server that runs open-source language models. Iris uses Ollama for all AI features: email classification, thread summarization, writing assist, and conversational chat. The server runs at the configured `OLLAMA_URL` (default: `http://localhost:11434`).

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
