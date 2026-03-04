---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
---

# Iris Agent Platform: Email API for AI Agents

## The Problem

AI agents are transforming how teams work — but when those agents need email access, you're stuck with vendor APIs. Gmail's API is Gmail-only. Outlook Graph is Microsoft-only. Both are cloud-hosted, rate-limited, and come with terms that can change without notice. There's no self-hosted, provider-agnostic email API built for the age of AI agents.

## The Solution

Iris ships with an open REST API that gives any AI agent secure access to email — across any provider, on your own infrastructure. Search messages, read threads, create drafts, send emails. Every action is scoped by permission level and recorded in a full audit log.

## Key Benefits

- **Provider-agnostic**: One API for Gmail, Outlook, Yahoo, Fastmail, and any IMAP server
- **Scoped permissions**: Four levels (read, draft, send, admin) — give agents exactly the access they need
- **Full audit trail**: Every API call logged with timestamp, action, IP address, and key identifier
- **AI-enriched data**: Responses include classification, priority, sentiment, and entity metadata
- **Self-hosted**: No rate limits you don't set. No vendor that can revoke access. No cloud dependency
- **Secure by design**: API keys are SHA-256 hashed, never stored in plaintext

## How It Works

1. **Generate an API key** in Iris settings with the permission level you need
2. **Connect your agent** using standard REST calls with bearer token authentication
3. **Automate with confidence**: Every action is logged, every permission is enforced

## Who It's For

- Engineering teams building AI agents that need email capabilities
- Teams automating customer support, sales outreach, or scheduling workflows
- Organizations evaluating email-as-a-platform for AI strategy
- Developers who need email automation beyond what GUIs provide

## Get Started

Deploy Iris, generate an API key, and point your agent at the REST API. Full documentation included.

**GitHub**: github.com/iris-email/iris

*The email API your AI agents have been waiting for.*
