---
id: ra-v0.4.0
type: release-announcement
audience: marketing
status: current
generated: 2026-03-26
hermes-version: 1.0.1
---

# Iris 0.4: Your Agent's Email API

Any AI agent can now plug into your email with a single API key. Iris 0.4 transforms the inbox from a human-only interface into a full programmable platform — 200+ API endpoints, permission-scoped access, and production-grade security.

---

## The Headline

Iris is no longer just an email client. It's an email API for AI agents. Generate a key, hand it to your agent, and it can search, read, compose, send, organize, and analyze email across every connected account. Claude-native agents connect instantly via MCP.

This is the missing piece for agent workflows. Your agent gets email superpowers without scraping UIs, hacking together IMAP scripts, or depending on cloud APIs you don't control.

---

## What Ships in v0.4.0

### Agent Platform — 200+ Endpoints

Iris exposes a comprehensive REST API that covers every email operation. Search messages, read threads, compose drafts, send replies, organize by category, analyze content. Each API key gets scoped permissions (read, draft, send, admin) and its own rate limit. Reply or forward a thread with a single API call. MCP support means Claude-powered agents connect with zero integration work.

### Semantic Search v5

Search by meaning, not keywords. "Find Sarah's email about the budget from last week" returns exactly what you need. Date range filtering narrows results to the right time window. Graph-aware ranking surfaces related context automatically — emails that connect to your query float to the top, even if you didn't explicitly ask for them.

### Production Hardened

Iris is ready for real deployments. The Docker image runs as a non-root user. Credentials are encrypted at rest. Security headers and CSRF protection are enabled by default. Rate limiting protects every endpoint. A CI/CD pipeline validates every release with 1,184 automated tests.

### Multi-Provider AI

Iris now supports multiple AI providers beyond Ollama. Bring the model that works best for your use case — the AI layer adapts.

---

## By the Numbers

- **200+** API endpoints for agent access
- **1,184** automated tests passing
- **4** permission levels for API key scoping
- **v5** Memories integration for semantic search
- **0** bytes sent to the cloud

---

## Getting Started

```bash
git clone https://github.com/iris-email/iris
cd iris
docker compose up
```

Open `http://localhost:3000`, add your email account, and generate an API key to connect your first agent.

---

## What's Next

Iris 0.4 establishes the agent platform foundation. Upcoming work includes webhook notifications for real-time agent triggers, agent-to-agent email workflows, and expanded MCP capabilities for deeper Claude integration.

---

*Iris is open source. Your email, your AI, your machine.*
