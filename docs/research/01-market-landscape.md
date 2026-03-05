# Iris — Market Landscape Research

> Date: 2026-03-01
> Status: Research complete

---

## 1. Global Email Market (2025-2026)

### Usage Stats
- **4.59 billion** email users worldwide in 2025 (56% of world population)
- Projected **4.73B by 2026**, **5.61B by 2030**
- **376.4 billion** emails sent daily in 2025 (~4% CAGR)
- Average user manages **1.86 email accounts**, receives **82-120 emails/day**
- Strongest growth: Asia-Pacific (4.2%), Middle East/Africa (5.1%)

### Market Size
- Email client software market: **$2.5B by 2033** (6.8% CAGR)
- Cloud email and collaboration: **$93B in 2025**
- Email hosting services: **$60.1B in 2024**, projected **$155.1B by 2030** (17.1% CAGR)

### Client Market Share (2025)
| Client | Share |
|--------|-------|
| Apple Mail | 48-56% |
| Gmail | 27-33% |
| Outlook | 4-7% |
| Others | ~10% |

Apple Mail + Gmail = ~84-90% of the market.

---

## 2. Key Startup Financials

| Company | Funding | Revenue/Valuation | Key Event |
|---------|---------|-------------------|-----------|
| **Superhuman** | $114M+ (a16z, IVP, Tiger Global) | $35M ARR, ~$825M valuation | Acquired by Grammarly (July 2025) |
| **Proton AG** | N/A (foundation-owned) | ~$97-102M revenue (2024), 100M+ users | Majority owned by Proton Foundation since June 2024 |
| **HEY** (37signals) | Bootstrapped | $99/yr; Basecamp overall $280M revenue (2024) | 7.47% YoY growth |
| **Shortwave** | $9M Series A (USV, Lightspeed) | N/A | Launched Tasklet automation (Oct 2025) |
| **Fastmail** | N/A | ~$159.6M estimated revenue | Recently raised prices |
| **Zero** (YC X25) | $500K SAFE (YC standard) | Open-source, MIT-licensed | Public beta at 0.email (July 2025) |
| **Stamp** (YC W25) | YC standard | First "AI-native" email client | Writes in user's voice, auto-prioritizes |

---

## 3. User Pain Points

### The Productivity Crisis
- Knowledge workers spend **28% of their workweek** (~11.7 hours) on email
- **35%** spend 2-5 hours daily reading/writing emails
- Email overload decreases productivity by up to **40%**
- Only **30%** of received emails require immediate action; **32%** go unread
- A 50-person company loses **1,250 hours/month** on unnecessary emails

### The Retention Threat
- **38%** of workers say email fatigue could make them quit
- **79%** blame constant emails/messages for workplace overwhelm
- Over a 45-year career, email management consumes **~3,000 working days**

### Gmail-Specific Complaints
- Spam filter not learning from manual spam marking
- Unsubscribe glitching (attempting to unsubscribe results in more mail)
- Nov 2024 Gemini AI training controversy — confusion about data handling

### Enterprise Pain Points
- **79%** of Microsoft 365 users faced cyber incidents; email = top attack vector
- **1 in 4** emails are malicious or unwanted spam
- **66%** of IT leaders say employee mistakes in outbound email cause more data loss than inbound attacks
- Inbox delivery rates dropped across major providers in early 2025

### What Power Users Want (That Doesn't Exist)
- Automatic surfacing of buried important items
- True "second brain" for email connecting context across threads
- Plain-English filter creation
- Native integration with broader workflows (CRM, PM, calendars)
- Unified view combining email with chat/messaging
- Better email deliverability for self-hosted/small servers

---

## 4. "Email Is Dead" vs "Email Is Evolving"

### Why People Say Email Is Obsolete
- Real-time tools: Teams hit **320M MAU** (145M DAU) by early 2024
- Chat offers seamless file sharing, presence indicators
- Email threads create information silos
- Gen Z prefers chat-native interfaces

### Why Email Persists
1. **Universality**: No shared platform needed
2. **Asynchronous by design**: Perfect for distributed teams
3. **Legal/compliance**: Business record in the eyes of the law
4. **Formal communication**: Required for external clients, contracts
5. **Persistence/searchability**: Messages persist indefinitely (Slack free tier deletes)
6. **Volume speaks**: Daily volume grew from 281B (2018) to 376B (2025)

### The Convergence Reality
- Trend is **coexistence**: email for official/external/formal; chat for quick internal
- Unified inbox tools (Front, Missive, Pylon) merging email + chat + SMS + social
- Only **13%** of companies carry context across channels
- Apple Mail and Gmail both adding AI features (summaries, smart replies)

---

## 5. AI + Email Opportunity

### Adoption
- **82%** of professionals use AI tools in their inbox daily
- **25%+** of inboxes use AI to summarize, categorize, or prioritize
- **40%+** of business users use smart reply/drafting tools weekly
- AI-assisted inbox management reduces response time by **18%**

### Privacy: The Real Barrier
- Public trust in AI companies protecting data: **47%** (down from 50% in 2023)
- **81%** think AI-collected info will be used in uncomfortable ways (Pew)
- **63%** concerned about generative AI compromising privacy (KPMG)
- AI incidents jumped **56.4%** in a year, **233 reported cases** in 2024

### AI-First vs AI-Assisted: A Real Distinction
- **AI-Assisted** (Superhuman): User in control, AI accelerates speed
- **AI-First** (Lindy, Stamp): AI takes proactive action — triages, drafts, follows up
- Market is splitting along this axis; both finding audiences

### The Gap Nobody Fills
- **82%** want AI in email, **81%** don't trust how data is handled
- No major player offers on-device/local AI with full agentic capabilities
- Apple is partial (3B on-device model), but limited
- This is THE whitespace: **privacy-preserving, AI-native email**

---

## 6. Self-Hosted / Privacy-First Trend

### Market Growth
- Self-hosted cloud storage: **$542M in 2025**, **$804.3M by 2030**
- Broader self-hosting market: **$85.2B by 2034**
- 2024 r/selfhosted survey: ~3,700 responses (2x previous year)
- **97%** of self-hosters use Docker

### Drivers
1. Data sovereignty
2. Cost reduction (cutting SaaS subscriptions)
3. Privacy backlash (AI training controversies)
4. Control and customization
5. Compliance (regulated industries)
6. Open source maturity (Docker, K8s, polished apps)

### Demographics
- Primarily developers and sysadmins with Linux experience
- Privacy-conscious professionals in regulated industries
- Home lab enthusiasts (Synology, Unraid, Proxmox)
- Growing more mainstream as tools simplify

### Self-Deployable as Product Strategy
- Proven models: GitLab (IPO), MongoDB, Elastic — open core + managed hosting
- **Zero (YC X25)** pursuing this for email: open-source, self-hostable
- Distribution via niche communities (Indie Hackers, Reddit, HN) often more effective than broad marketing

---

## 7. Key Takeaways

1. **Enormous, growing market** — 4.59B users, $93B cloud email market — but the client layer ($2.5B) is undermonetized
2. **Superhuman exit validates the space** — $825M acquisition confirms premium email has strategic value
3. **Biggest unserved gap: privacy-preserving AI email** — 82% want AI, 81% worried about data
4. **Self-hosted + AI-native is whitespace** — Zero (YC X25) is first entrant but early-stage
5. **Email fatigue is a retention crisis** — 38% would quit over overload
6. **Convergence is underexploited** — only 13% carry context across channels
