---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
---

# Iris: AI-Native Email That Runs on Your Machine

## The Problem

Email is the backbone of professional communication, yet email clients are stuck in the past. The tools that do add AI intelligence — Superhuman, Shortwave, Gmail's built-in AI — require you to hand your most sensitive data to cloud servers you don't control. For privacy-conscious professionals and regulated organizations, that's a non-starter.

## The Solution

Iris is a self-hosted email client with built-in AI that runs entirely on your machine. Connect any email provider. Get automatic classification, semantic search, AI chat, and writing assistance — all processed locally through your own AI models. No cloud. No subscriptions. No data leaves your hardware.

## Key Benefits

- **AI that respects your privacy**: Classification, summarization, search, and chat powered by local Ollama models
- **Works with every provider**: Gmail, Outlook, Yahoo, Fastmail, any IMAP server — one client for all accounts
- **Agent-ready**: Open REST API with scoped permissions lets AI agents automate email workflows on your infrastructure
- **Semantic search**: Find emails by meaning, not just keywords. Ask for concepts and get results
- **Deploy in minutes**: One Docker Compose command. No infrastructure team required

## How It Works

1. **Deploy**: Run `docker compose up` to start Iris and a local AI runtime
2. **Connect**: Add your email accounts via OAuth2 (Gmail, Outlook) or IMAP credentials
3. **Work smarter**: Every email is classified, searchable, and queryable through AI — all locally

## Who It's For

- Privacy-conscious professionals managing sensitive communications
- Organizations in regulated industries (legal, healthcare, finance)
- Developers and teams building AI-powered email automation
- Power users who want AI productivity without surveillance tradeoffs

## Get Started

Iris is open source and free. Clone the repository, run Docker Compose, and connect your accounts.

**GitHub**: github.com/iris-email/iris

*Your email, your AI, your machine.*
