---
id: fb-024
type: feature-brief
audience: marketing
topic: agent-platform
status: current
generated: 2026-03-26
hermes-version: 1.0.1
---

# Agent Platform

## One-Liner

Iris is a full email API for AI agents — 200+ endpoints, permission-scoped access, and one-call replies, all behind a single API key.

## The Problem

AI agents are transforming how work gets done, but email remains a walled garden. Today, connecting an agent to email means scraping web UIs, reverse-engineering undocumented APIs, or stitching together fragile IMAP scripts. The result is brittle, insecure, and impossible to scale.

## The Solution

Iris turns your email into a programmable platform. Any AI agent authenticates with a single API key and immediately accesses 200+ endpoints spanning search, read, compose, send, organize, and analyze. Permission scoping ensures agents only touch what they should. Reply or forward with one API call. Claude-native agents connect instantly via MCP.

## Key Benefits

- **Instant email superpowers for any agent**: 200+ endpoints covering every email operation — search, read, compose, send, organize, and analyze
- **Permission-scoped security**: Four access levels (read, draft, send, admin) let you give each agent exactly the access it needs
- **One-call actions**: Reply to a thread, forward a message, or send a new email in a single API call — no multi-step orchestration
- **Per-key rate limiting**: Each API key gets its own rate limit, so one runaway agent never impacts another
- **MCP-native for Claude agents**: Claude-powered agents connect through MCP with zero additional integration work

## How It Works (Simple)

Generate an API key in Iris with the permissions your agent needs. Hand that key to your agent — it now has full, scoped access to your email through a clean REST API. For Claude-based agents, point them at Iris via MCP and they connect natively. Every action is logged, every key is rate-limited, and you can revoke access instantly.

## Suggested Messaging

**Headline**: "Your AI agent just got email. 200+ endpoints. One API key. Zero scraping."

**Product launch**: "Iris 0.4 turns your inbox into an agent platform. Any AI agent can search, read, compose, and send email through a secure, scoped API — no hacks, no workarounds, no cloud dependency."

**Developer pitch**: "Stop building email integrations from scratch. Iris gives your agent 200+ email endpoints, permission scoping, per-key rate limits, and MCP support out of the box."

## Competitive Edge

No other self-hosted email client offers a full agent-ready API at this scale. Gmail and Outlook APIs require cloud accounts, enforce strict rate limits, and lock you into a single provider. Iris runs on your infrastructure, works across every email provider, and gives you complete control over what each agent can access. MCP support makes Iris the first email platform purpose-built for the Claude agent ecosystem.
