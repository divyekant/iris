---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
---

# Iris Product Datasheet

**AI-Native, Self-Hosted Email Client**
*Your email, your AI, your machine.*

---

## Overview

Iris is a self-hosted email client with built-in AI intelligence. It connects to any email provider, processes every message with local AI, and exposes an open API for agent automation — all without sending a single byte of your data to the cloud.

---

## Key Capabilities

### Email Management

- **Universal provider support**: Gmail, Outlook, Yahoo, Fastmail, any IMAP provider
- **OAuth2 authentication**: Secure sign-in for Gmail and Outlook with automatic token refresh
- **Real-time sync**: IMAP IDLE delivers new emails within seconds
- **Threading**: Automatic conversation threading via References and In-Reply-To headers
- **Compose**: New messages, replies, reply-all, and forwards with draft auto-save
- **Batch actions**: Select and process multiple messages at once (archive, categorize, mark read)
- **Category views**: Primary, social, promotions, updates, forums — with tab navigation
- **Full-text search**: SQLite FTS5 with snippet highlighting, date filters, attachment filters

### AI Intelligence

- **Automatic classification**: Every email categorized by type (primary, social, promotions, updates, forums, spam)
- **Priority detection**: AI assigns urgency levels so you see what matters first
- **Sentiment analysis**: Understand the tone of incoming messages at a glance
- **Entity extraction**: Dates, amounts, names, and action items pulled out automatically
- **Thread summarization**: One-click summaries of long email threads
- **AI chat**: Ask questions about your email in natural language, get cited answers
- **Writing assist**: AI-powered rewrite, formalize, casualize, shorten, and lengthen for compose
- **Feedback loop**: Correct AI mistakes and the system learns from your input
- **Semantic search**: Find emails by meaning using local vector search

### Agent Platform

- **Open REST API**: Search, read, draft, and send emails programmatically
- **Scoped API keys**: Four permission levels (read, draft, send, admin)
- **SHA-256 key security**: API keys are hashed — never stored in plaintext
- **Full audit logging**: Every API action recorded with timestamp, IP, action, and key identifier
- **AI-enriched responses**: API returns include classification, priority, and entity metadata

### Privacy and Security

- **Local-first architecture**: All data stored on your machine in SQLite
- **No cloud dependency**: No Iris servers, no telemetry, no third-party data sharing
- **Session authentication**: Secure local session tokens for the web interface
- **Trust indicators**: SPF, DKIM, and DMARC validation displayed per message
- **Tracking pixel detection**: Identifies and flags email tracking pixels
- **Bring your own AI**: Connect any Ollama-compatible model

---

## Technical Specifications

| Component | Technology |
|---|---|
| Backend | Rust, Axum 0.8 |
| Database | SQLite (rusqlite, bundled) |
| Search | SQLite FTS5 + Memories vector store |
| Frontend | Svelte 5, TypeScript, Vite 7, Tailwind CSS 4 |
| AI Runtime | Ollama (local inference) |
| IMAP | async-imap with IDLE support |
| SMTP | lettre 0.11 with XOAUTH2 |
| Auth | OAuth2 v5 (Gmail, Outlook), session tokens, API keys |
| Packaging | Docker Compose |

---

## Dependencies

| Dependency | Purpose | Required |
|---|---|---|
| Ollama | Local AI inference (classification, summarization, chat, writing) | Optional (AI features disabled without it) |
| Memories MCP | Semantic vector search | Optional (falls back to FTS5 keyword search) |

---

## Integrations

| Provider | Authentication | Status |
|---|---|---|
| Gmail | OAuth2 | Supported |
| Outlook / Microsoft 365 | OAuth2 | Supported |
| Yahoo Mail | IMAP credentials | Supported |
| Fastmail | IMAP credentials | Supported |
| ProtonMail (via Bridge) | IMAP credentials | Supported |
| Any IMAP server | IMAP credentials | Supported |

---

## Deployment

### Docker Compose (Recommended)

```bash
docker compose up
```

Starts Iris backend and Ollama sidecar. Fully operational in under 2 minutes.

### Native Binary

Build from source with Rust 1.85+ and Node.js 20+.

---

## Security Model

| Layer | Mechanism |
|---|---|
| Web UI authentication | Session tokens |
| API authentication | Bearer tokens (SHA-256 hashed keys) |
| Email authentication | OAuth2 (Gmail, Outlook), IMAP credentials |
| Email trust validation | SPF, DKIM, DMARC header parsing |
| Privacy protection | Tracking pixel detection |
| Audit | Full API action logging (timestamp, IP, action, key) |

---

## System Requirements

| Requirement | Minimum |
|---|---|
| Rust | 1.85+ |
| Node.js | 20+ |
| Ollama | Latest (optional, for AI features) |
| RAM | 4 GB (8 GB recommended with AI) |
| Disk | 500 MB + email storage |
| OS | Linux, macOS, Windows (via Docker) |

---

## Version

**Iris v0.1.0** | Released 2026-03-04 | 12 feature areas | 78 tests passing
