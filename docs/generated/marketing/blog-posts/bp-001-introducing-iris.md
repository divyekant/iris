---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
---

# Email Deserves Better Than This

You check your inbox 74 times a day. You spend 28% of your workweek managing email. And your email client — the tool you use more than almost any other — hasn't fundamentally changed since Gmail launched in 2004.

Sure, there have been improvements. Superhuman made email fast. HEY gave it opinions. Shortwave added AI summaries. But they all share the same architecture: your email goes to their cloud, their AI processes it on their servers, and you trust that they're handling your most sensitive professional communications responsibly.

81% of professionals say they're uncomfortable with that arrangement. We think they're right.

## The Tradeoff That Shouldn't Exist

Today, if you want AI in your email, you pay a tax: your privacy. Superhuman processes your messages on AWS at $30 per month. Shortwave runs your email through Google's AI. Even Gmail's native Gemini features analyze your conversations on Google's infrastructure.

For anyone handling sensitive information — lawyers, doctors, executives, financial advisors, or simply people who value privacy — this creates an impossible choice. Get the AI productivity gains, or keep your data private. Pick one.

We built Iris because we believe that choice is unnecessary.

## Introducing Iris

Iris is an AI-native email client that runs entirely on your machine. Local email storage. Local AI processing. Local search. No cloud service, no telemetry, no data sharing. You deploy it on your hardware, connect your email accounts, and everything stays under your control.

But Iris isn't just a privacy play. It's a genuinely capable email client with AI intelligence woven into every interaction.

**Every email gets smarter the moment it arrives.** Iris classifies incoming messages by category and priority, detects sentiment, and extracts key entities like dates, dollar amounts, and action items. The AI runs in the background through Ollama — you see the results as priority badges and category labels in your inbox.

**Search understands what you mean.** Traditional email search is literal: search for "budget" and miss the email that says "financial plan." Iris adds semantic search powered by a local vector store. Search by concept, not just keywords.

**Ask your email anything.** Open the chat panel and type a question in plain English. "What did the vendor quote for the renewal?" Iris retrieves relevant emails, generates an answer, and cites its sources. If the AI suggests an action — like drafting a reply — you confirm before anything happens.

**Write with AI assistance.** Rewrite, formalize, casualize, shorten, or lengthen your drafts. The AI runs locally, so your draft content stays private.

**Connect any provider.** Gmail, Outlook, Yahoo, Fastmail, your self-hosted server — Iris speaks standard IMAP and SMTP. OAuth2 for major providers, real-time sync via IMAP IDLE. One client for all your accounts.

**Build on top of it.** Iris ships with an open REST API for AI agents. Create API keys with scoped permissions, connect your automation framework, and let agents search, read, draft, and send email. Every action is logged in a full audit trail.

## What Makes Iris Different

The market has no shortage of email clients. Here's why Iris occupies a unique position:

**Self-hosted and local-first.** Unlike Superhuman, HEY, Shortwave, and Gmail, Iris has no cloud service. There are no servers to trust, no privacy policies to parse, no terms that can change. You run it, you own it.

**AI-native, not AI-bolted.** AI isn't a feature tab in Iris — it's the foundation. Every email is processed, every search is enhanced, every interaction is informed by local intelligence.

**Provider-agnostic.** Most AI email clients only work with Gmail. Iris works with any IMAP provider, giving you AI intelligence regardless of which service hosts your email.

**Agent-ready.** Iris is the first self-hosted email client with a complete agent API — scoped permissions, audit logging, and AI-enriched responses out of the box.

**Open source.** Inspect, modify, and verify every component. No black boxes.

## Built for This Moment

There are 4.59 billion email users worldwide. The email client market is projected to reach $2.5 billion by 2033. 82% of professionals are already using AI in their email workflows.

The demand is clear. The gap in the market is equally clear: nobody has built a self-deployable, local AI-native email client with universal provider support and an open agent API.

Iris fills that gap.

## What Ships Today

Iris v0.1.0 includes 12 feature areas built across 10 vertical slices: email management, threading, compose, batch actions, full-text and semantic search, AI classification, summarization, chat, writing assist, agent API, trust indicators, and a semantic memory layer. 78 tests pass. Zero bytes go to the cloud.

## Get Started

Iris is open source and free. No account to create. No credit card required.

```bash
git clone https://github.com/iris-email/iris
cd iris
docker compose up
```

Open your browser, add your email accounts, and experience what email should have been all along.

Your email. Your AI. Your machine.
