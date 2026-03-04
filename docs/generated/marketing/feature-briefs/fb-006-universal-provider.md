---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
---

# Feature Brief: Universal Provider Support

## One-Liner

One email client for all your accounts — Gmail, Outlook, Yahoo, Fastmail, or any IMAP provider.

## What It Is

Iris connects to any email provider that supports IMAP and SMTP — the universal email protocols. Add your Gmail work account, your Outlook personal account, and your custom domain email, and manage them all from a single interface. OAuth2 authentication for Gmail and Outlook, standard credentials for everything else.

## Who It's For

- Professionals juggling multiple email accounts across different providers
- Freelancers and consultants managing client and personal email separately
- Organizations migrating between providers who need a client that doesn't care which server you use
- Anyone tired of being locked into one provider's ecosystem

## The Problem It Solves

Most modern email clients are Gmail-only (Superhuman, Shortwave, Zero) or tied to a specific ecosystem (Apple Mail for iCloud, Outlook for Microsoft 365). If you use multiple providers, you're stuck switching between apps or settling for a lowest-common-denominator experience. Iris treats every provider as a first-class citizen.

## Key Benefits

1. **Unified inbox**: See all your accounts in one view. Switch between them instantly with the account switcher
2. **Any IMAP provider**: Gmail, Outlook, Yahoo, Fastmail, ProtonMail Bridge, self-hosted — if it speaks IMAP, Iris works with it
3. **OAuth2 where it matters**: Secure, modern authentication for Gmail and Outlook. No app passwords required
4. **Real-time sync**: IMAP IDLE keeps your inbox current. New emails appear within seconds of arrival
5. **No provider lock-in**: Your AI intelligence, search index, and agent API work identically across all accounts

## How It Works

In Iris settings, add an email account by selecting your provider. For Gmail and Outlook, Iris walks you through OAuth2 authentication — secure, no password stored. For other providers, enter your IMAP/SMTP credentials. Iris connects, syncs your inbox, and starts AI processing immediately. Add as many accounts as you need and switch between them with one click.

## Competitive Context

| Capability | Iris | Superhuman | HEY | Shortwave | Zero | Thunderbird |
|---|---|---|---|---|---|---|
| Gmail | Yes | Yes | Via IMAP | Yes | Yes | Yes |
| Outlook | Yes | No | Via IMAP | No | No | Yes |
| Any IMAP | Yes | No | No | No | No | Yes |
| OAuth2 | Yes | Yes | No | Yes | No | Yes |
| IMAP IDLE | Yes | N/A | N/A | N/A | N/A | Yes |
| AI across providers | Yes | Gmail only | No AI | Gmail only | Gmail only | No AI |
| Unified inbox | Yes | No | No | No | No | Yes |

## Proof Points

- OAuth2 with automatic token refresh for Gmail and Outlook
- IMAP IDLE with 29-minute timeout for real-time sync
- XOAUTH2 SASL authentication for secure SMTP sending
- Account switcher with per-account category views

## Suggested Messaging

**Announcement**: "Iris works with every email provider — Gmail, Outlook, Yahoo, Fastmail, or your self-hosted server. One client, all your accounts, full AI intelligence everywhere."

**Sales pitch**: "Why should your email client dictate which provider you use? Iris connects to any IMAP provider with full AI features across every account — no compromises, no lock-in."

**One-liner**: "Every email account. One intelligent client."
