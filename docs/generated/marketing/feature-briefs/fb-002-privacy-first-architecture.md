---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
---

# Feature Brief: Privacy-First Architecture

## One-Liner

Everything runs on your machine. No cloud. No telemetry. No data leaves your control.

## What It Is

Iris is a fully self-hosted email client. Your emails are stored in a local SQLite database. AI processing runs through Ollama on your own hardware. Authentication happens locally. There is no Iris cloud service, no analytics pipeline, no third-party data sharing. You own every byte.

## Who It's For

- Security-conscious professionals and executives handling sensitive communications
- Organizations in regulated industries (legal, healthcare, finance) that cannot send email data to third parties
- Privacy advocates who want AI productivity without surveillance capitalism
- IT teams that need auditable, self-hosted infrastructure

## The Problem It Solves

81% of professionals are uncomfortable with how AI tools handle their email data. Every major AI email product — Superhuman, Shortwave, even Gmail's built-in AI — processes your messages on cloud servers you don't control. For regulated industries, this creates compliance risk. For everyone else, it means trusting a company's privacy promise instead of verifying it yourself.

## Key Benefits

1. **True local-first**: Email, AI models, search indexes, and configuration all live on your machine. Nothing phones home
2. **Bring your own AI**: Connect any Ollama-compatible model. Swap models anytime. No vendor lock-in on intelligence
3. **Auditable by design**: The entire codebase is open. You can inspect, modify, and verify every component
4. **Zero trust required**: You don't need to trust Iris with your data — because Iris never sees it. You run it, you own it
5. **Compliance-ready**: Data residency is solved by default. Your email stays exactly where your machine is

## How It Works

Deploy Iris with a single Docker Compose command. It starts a local Rust backend (your email engine) and an Ollama instance (your AI). Iris connects to your email providers via standard IMAP/SMTP — the same protocols your current client uses. All data is stored locally in SQLite. AI inference runs locally through Ollama. There is no cloud relay, no proxy, no intermediary.

## Competitive Context

| Capability | Iris | Superhuman | HEY | Shortwave | Zero |
|---|---|---|---|---|---|
| Self-hosted | Yes | No | No | No | Partial |
| Local AI | Yes | No | No | No | No |
| No cloud dependency | Yes | No | No | No | No |
| Open source | Yes | No | No | No | Yes |
| Data residency | Your machine | AWS | AWS | Google Cloud | Cloud |
| Telemetry | None | Yes | Yes | Yes | Unknown |

## Proof Points

- Zero outbound network calls except to your configured email providers
- SQLite database is a single file you can back up, move, or delete at will
- Docker Compose deployment: one command, fully operational in under 2 minutes
- SPF/DKIM/DMARC validation built in — trust indicators without cloud dependencies

## Suggested Messaging

**Announcement**: "Iris is the first AI email client where everything — email storage, AI processing, search — runs entirely on your machine. No cloud. No compromise."

**Sales pitch**: "Your legal team's emails shouldn't live on someone else's servers. Iris gives you AI-powered email intelligence with complete data sovereignty — deploy it on your infrastructure, connect your own AI, and never wonder where your data goes."

**One-liner**: "AI email intelligence with zero cloud dependency."
