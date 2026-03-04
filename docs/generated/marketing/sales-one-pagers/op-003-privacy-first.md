---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
---

# Iris: AI Email Intelligence With Zero Cloud Dependency

## The Problem

Every AI-powered email tool on the market processes your messages on cloud servers. Superhuman stores your data on AWS. Shortwave runs through Google's AI. Gmail's Gemini features analyze your email on Google's infrastructure. For professionals handling sensitive communications — legal, financial, medical, executive — this creates unacceptable risk. 81% of professionals say they're uncomfortable with how AI tools handle their data.

## The Solution

Iris is an AI-powered email client where everything runs on your machine. Email storage, AI processing, search indexing, and the entire application stack — all local. No Iris cloud service exists. No telemetry is collected. No data leaves your hardware. You get AI classification, semantic search, conversational chat, and writing assistance with complete data sovereignty.

## Key Benefits

- **Truly local**: All data stored in SQLite on your machine. AI runs through Ollama on your hardware
- **No telemetry**: Zero outbound network calls except to your configured email providers
- **Auditable**: Open-source codebase you can inspect, modify, and verify
- **Compliance-ready**: Data residency solved by default — your email stays where your machine is
- **Trust indicators**: SPF, DKIM, and DMARC validation built into the reading experience
- **Tracking protection**: Email tracking pixels are detected and flagged automatically

## How It Works

1. **Deploy on your infrastructure**: One Docker Compose command starts Iris and a local AI runtime
2. **Connect your accounts**: OAuth2 for Gmail and Outlook, standard IMAP for everything else
3. **Work with confidence**: Every AI feature runs locally. Verify by inspecting the open-source code

## Who It's For

- Legal professionals handling privileged communications
- Healthcare organizations subject to data protection requirements
- Financial services teams managing sensitive client correspondence
- Executives with confidential communications
- Anyone who believes privacy is not a feature — it's a right

## Get Started

Iris is open source and free. No account creation. No credit card. No data collection.

**GitHub**: github.com/iris-email/iris

*Your email, your AI, your machine. Nothing leaves.*
