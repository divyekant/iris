---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
---

# Feature Brief: AI Email Intelligence

## One-Liner

Iris automatically classifies, prioritizes, and extracts key information from every email — using AI that runs entirely on your machine.

## What It Is

Iris applies local AI to every incoming email, sorting it into categories (primary, social, promotions, updates, forums, spam), assigning priority levels, detecting sentiment, and extracting entities like dates, dollar amounts, and action items. When the AI gets it wrong, you correct it — and it learns from your feedback.

## Who It's For

- Professionals managing 100+ emails per day who need signal from noise
- Privacy-conscious users who refuse to send email data to cloud AI services
- Teams that want AI-powered email triage without vendor lock-in

## The Problem It Solves

Email overload costs knowledge workers an estimated 28% of their workweek. Existing AI tools (Superhuman, Shortwave, Gmail's AI) process your email on someone else's servers. You're forced to choose: productivity or privacy. Iris eliminates that tradeoff.

## Key Benefits

1. **Instant triage**: Every email is classified and prioritized the moment it arrives — no manual sorting
2. **Complete privacy**: AI processing happens on your machine using Ollama. Your email never leaves your hardware
3. **Self-improving accuracy**: Correct a misclassification once, and Iris learns. The AI adapts to your communication patterns
4. **Actionable extraction**: Key dates, amounts, names, and action items are surfaced automatically — no reading between the lines
5. **Zero subscription cost**: No per-seat fees for AI features. Run your own models, pay nothing

## How It Works

When a new email arrives, Iris sends it through a local AI pipeline powered by Ollama. In a single pass, the AI classifies the email by category and priority, detects the sender's tone, and pulls out important details. Results appear as priority badges and category labels in your inbox. If something looks off, one click corrects it — and Iris remembers.

## Competitive Context

| Capability | Iris | Superhuman | Shortwave | Gmail | Zero |
|---|---|---|---|---|---|
| AI classification | Local | Cloud | Cloud (Google) | Cloud | Cloud |
| Priority detection | Yes | Yes | Yes | Limited | Yes |
| Entity extraction | Yes | No | Limited | No | Limited |
| Feedback learning | Yes | No | No | No | No |
| Data stays local | Yes | No | No | No | No |
| Cost | Free | $30/mo | $7/mo | Free* | Free |

## Proof Points

- 4 AI processing modes (classification, summarization, chat, writing assist) in a single local pipeline
- Single-prompt classification produces category, priority, sentiment, and entities in one inference call
- Background processing on sync — no user wait time

## Suggested Messaging

**Announcement**: "Iris now classifies, prioritizes, and extracts key details from every email — automatically, privately, and on your own hardware."

**Sales pitch**: "What if your email client understood every message before you opened it? Iris uses local AI to sort, prioritize, and surface what matters — without sending a single byte to the cloud."

**One-liner**: "AI email triage that never leaves your machine."
