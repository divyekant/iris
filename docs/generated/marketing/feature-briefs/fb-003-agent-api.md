---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
---

# Feature Brief: Agent API

## One-Liner

An open REST API that lets any AI agent search, read, draft, and send emails — with scoped permissions and full audit logging.

## What It Is

Iris exposes a complete email API that external AI agents and automations can use. Create API keys with fine-grained permissions (read-only, draft, send, admin), connect any agent framework, and let it work with your email. Every action is logged with timestamps, IP addresses, and key identifiers. You stay in control.

## Who It's For

- Developers building AI agents that need email capabilities
- Teams automating email workflows (customer support, sales outreach, scheduling)
- Organizations evaluating email-as-a-platform for their AI strategy
- Power users who want to script their email beyond what GUIs allow

## The Problem It Solves

Building email automation today means either using Gmail/Outlook APIs (vendor lock-in, rate limits, cloud-only) or hacking together IMAP scripts (fragile, no AI layer). There's no standard way to give an AI agent secure, scoped access to email with built-in intelligence. Iris fills that gap.

## Key Benefits

1. **Open and standard**: REST API with JSON responses. Works with any language, any agent framework, any automation tool
2. **Scoped security**: Four permission levels (read, draft, send, admin) let you give agents exactly the access they need — nothing more
3. **Full audit trail**: Every API call is logged. See which agent did what, when, and from where
4. **AI-enriched responses**: API responses include AI metadata (categories, priorities, entities) — agents get intelligence for free
5. **Self-hosted control**: Unlike Gmail API or Outlook Graph, there are no rate limits you don't set, no terms that can change, no vendor that can revoke access

## How It Works

Generate an API key in Iris settings with the permissions you need. Hand that key to your agent or automation script. The agent authenticates with a bearer token and accesses endpoints for searching messages, reading threads, creating drafts, and sending emails. Iris handles IMAP/SMTP complexity behind the scenes. Every action is recorded in the audit log, visible in the Iris UI.

## Competitive Context

| Capability | Iris | Gmail API | Outlook Graph | Superhuman | Zero |
|---|---|---|---|---|---|
| Open REST API | Yes | Yes | Yes | No | Limited |
| Self-hosted | Yes | No | No | No | Partial |
| Scoped API keys | Yes | OAuth scopes | OAuth scopes | N/A | No |
| Audit logging | Yes | Cloud logs | Cloud logs | No | No |
| AI-enriched data | Yes | No | No | No | Limited |
| No rate limits | Yes | Strict | Strict | N/A | Unknown |
| Any provider | Yes | Gmail only | Outlook only | Gmail only | Gmail only |

## Proof Points

- 4 permission levels with SHA-256 key hashing
- Full audit log with timestamp, action, IP, and key tracking
- Agent endpoints: search, read message, read thread, create draft, send email
- Trust indicators (SPF/DKIM/DMARC) available in API responses

## Suggested Messaging

**Announcement**: "Iris ships with an open Agent API — give any AI agent secure, audited access to search, read, draft, and send email across all your accounts."

**Sales pitch**: "Your AI agent needs email. Iris gives it a clean REST API with scoped permissions, audit logging, and built-in intelligence — all running on your infrastructure, across any email provider."

**One-liner**: "The email API your AI agents have been waiting for."
