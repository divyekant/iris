---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
---

# Iris v0.1.0: AI-Powered Email That Never Leaves Your Machine

Today we're releasing Iris, an open-source email client that brings AI intelligence to your inbox without compromising your privacy. Everything runs locally — your emails, your AI, your data.

---

## The Problem

Email clients haven't meaningfully evolved in a decade. The new wave of AI-powered email tools (Superhuman, Shortwave, Gmail's Gemini) offer genuine productivity gains — but they process your most sensitive communications on cloud servers you don't control.

81% of professionals are uncomfortable with how AI tools handle their data. We think they're right to be.

## A Better Approach

Iris runs entirely on your machine. No cloud service. No telemetry. No data sharing. You get the full power of AI email intelligence with complete data sovereignty.

---

## What Ships in v0.1.0

### AI Email Intelligence

Every incoming email is automatically classified by category and priority, analyzed for sentiment, and scanned for key entities (dates, amounts, action items). Correct a mistake, and Iris learns. All processing runs through Ollama on your hardware.

### Semantic Search

Search your email by meaning, not just keywords. Ask for "emails about rescheduling the team offsite" and find relevant messages even when the exact words differ. Powered by a local Memories vector store with automatic fallback to full-text search.

### AI Chat with Citations

Open the chat panel and ask questions about your email in natural language. "What did the vendor quote for the annual contract?" Iris retrieves relevant emails, generates an answer, and cites its sources. Propose actions like drafting a reply — with your explicit confirmation before anything happens.

### Writing Assist

Compose emails with AI assistance. Rewrite, formalize, casualize, shorten, or lengthen your drafts with one click. The AI runs locally, so your draft content stays private.

### Agent API

An open REST API lets external AI agents and automations interact with your email. Create API keys with scoped permissions (read, draft, send, admin). Every action is logged in a full audit trail. Build email-powered agents on your own infrastructure.

### Universal Provider Support

Iris connects to Gmail, Outlook, Yahoo, Fastmail, and any IMAP provider. OAuth2 for major providers, IMAP IDLE for real-time sync, XOAUTH2 for secure SMTP. One client for all your accounts.

### Trust and Safety

SPF, DKIM, and DMARC validation shows you which emails can be trusted. Tracking pixel detection flags messages that are monitoring your opens. Security built into the reading experience.

---

## By the Numbers

- **12** feature areas across 10 vertical slices
- **78** tests passing
- **60** commits on main
- **4** AI modes: classification, summarization, chat, writing assist
- **0** bytes sent to the cloud

---

## Getting Started

```bash
git clone https://github.com/iris-email/iris
cd iris
docker compose up
```

Open `http://localhost:3000`, add your email account, and Iris handles the rest.

---

## What's Next

Iris v0.1.0 is the foundation. Upcoming work includes:

- Backend hardening and performance optimization
- End-to-end test coverage
- Additional AI model support
- Deployment packaging improvements
- Community feedback integration

---

## Built With

Rust, Axum, SQLite, Svelte 5, Tailwind CSS, Ollama, Docker Compose.

---

*Iris is open source. Your email, your AI, your machine.*
