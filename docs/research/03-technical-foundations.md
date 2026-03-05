# Iris — Technical Foundations Research

> Date: 2026-03-01
> Status: Research complete

---

## 1. Email Protocols — Current State

### IMAP vs JMAP

**JMAP** (JSON Meta Application Protocol) — RFC 8620/8621. Modern replacement for IMAP + SMTP submission.

| Aspect | IMAP | JMAP |
|--------|------|------|
| Initial sync (3 folders) | 7+ round trips | 1 round trip |
| Re-sync with changes | Multiple calls per folder | 1 bundled call |
| Connection model | Persistent TCP (battery drain) | Stateless HTTP (mobile-friendly) |
| Attachment upload | Re-sends entire email content | Separate blob upload |
| Parser complexity | Custom text protocol | Standard JSON |
| Push notifications | IDLE (one folder only) | Built-in EventSource + Web Push |

**JMAP Server Support (Production):**
- Fastmail (primary driver of the spec)
- Stalwart Mail Server (open source, JMAP-first)
- Cyrus IMAP (provisional since v3.8.3)
- Apache James (since v3.6.0)

**Reality:** Gmail, Outlook, Yahoo don't support JMAP. IMAP + SMTP remains baseline. JMAP = optional fast path for compatible servers.

### SMTP Changes

- No replacement for server-to-server delivery
- JMAP replaces only client-to-server submission
- Key extensions: STARTTLS, SMTP AUTH with OAuth2, ARC, MTA-STS

### OAuth2 — Now Mandatory

| Provider | Status | Key Details |
|----------|--------|-------------|
| **Gmail** | OAuth2 enforced March 14, 2025 | Requires app verification + annual security audit for restricted scopes. 15 concurrent IMAP connections. POP discontinued Q1 2026. |
| **Microsoft** | Consumer OAuth2 enforced May 5, 2025. EWS shutdown Oct 2026. | New Outlook removed POP/IMAP entirely. Must migrate to Graph API. |
| **Yahoo** | OAuth2 or app-specific passwords | 5 concurrent IMAP per IP. Most restrictive. |

**Practical implication:** Building a third-party client requires implementing OAuth2 per provider, managing token refresh, and potentially passing Google's security audit.

### Email Authentication (SPF/DKIM/DMARC/BIMI)

A **receiving** client should surface Authentication-Results headers:
- SPF: sending server IP authorized?
- DKIM: cryptographic signature valid?
- DMARC: alignment between SPF/DKIM and From header?
- BIMI: brand logo display for DMARC-passing senders

Gmail, Yahoo, Microsoft (2025) require bulk senders to have SPF + DKIM + DMARC. One-click unsubscribe mandatory for bulk mail.

---

## 2. Local/On-Device AI for Email

### Model Requirements by Task

| Task | Min Model Size | Recommended Models |
|------|---------------|-------------------|
| Classification/categorization | 1-3B | Phi-3 Mini (3.8B), Gemma 2B, Qwen2 1.5B |
| Summarization | 3-7B | Phi-3.5 Mini (3.8B), Qwen2-7B, Gemma 9B |
| Smart reply / draft generation | 7B+ | Llama 3.1 8B, Mistral 7B, Qwen2.5-7B |
| Natural language search | 3-7B | Phi-3 Mini + embedding models (nomic-embed) |
| Priority scoring | 1-3B | Fine-tuned classifier or small LLM |

**Apple Intelligence reference:** ~3B model with 2-bit quantization on Neural Engine. Handles categorization, summarization, priority on-device.

### Inference Frameworks

| Framework | Best For | Speed | Cross-Platform |
|-----------|---------|-------|---------------|
| **MLX** | macOS (Apple Silicon) | Fastest (20-50% over llama.cpp) | Apple only |
| **llama.cpp** | Cross-platform, embedded | Excellent | Yes |
| **Ollama** | Developer ergonomics (REST API) | Good (built on llama.cpp) | Yes |
| **PyTorch MPS** | Not recommended | Memory issues | Limited |

**Hardware Reality:**
- 8GB RAM: 3-4B models (Phi-3 Mini 4-bit)
- 16GB RAM: 7-8B models (Llama 3.1 8B 4-bit)
- 32GB+: 12-27B models
- Qwen3-8B: 25+ tokens/sec on M3 Pro with 12GB VRAM

### Privacy Implications

**Local AI:**
- All email stays on-device
- No API keys, tracking, telemetry
- Works offline, no per-query cost
- Model updates manual

**Recommended hybrid approach (Apple's model):** Simple tasks on-device (3B), complex tasks routed to privacy-preserving backend.

### Existing Local AI + Email Projects

| Project | Architecture | AI Approach |
|---------|-------------|-------------|
| **Velo** | Tauri v2 + Rust + React 19, SQLite + FTS5 (33 tables) | Cloud APIs (Claude, GPT, Gemini) with local cache |
| **Inbox Zero** | Next.js, PostgreSQL + Redis, Gmail API | Multiple LLM providers including **Ollama** |
| **Mail0 / Zero** | Open source, self-hostable | Privacy-first, multiple providers |
| **Openmail** | Desktop, no cloud dependencies | Privacy-focused, local only |

---

## 3. Architecture Patterns

### Real-Time Sync

**WebSocket bridge (best for email):**
```
Browser <--WebSocket--> Backend Proxy <--IMAP IDLE--> Mail Server
```
Reference: wssmail project — WebSocket bridges to IMAP IDLE for sub-second notifications.

**Other options:**
- SSE: for JMAP (EventSource built into spec)
- Polling: fallback for corporate firewalls (30-60s interval)
- Gmail: Google Pub/Sub webhooks for server-to-server

### Offline Support

**Storage:**
- IndexedDB: browser's transactional DB (use Dexie.js for friendlier API)
- SQLite in browser: via sql.js (WASM) or OPFS-backed
- For desktop (Tauri): native SQLite + FTS5 (Velo uses 33 tables)

**Service Workers:** Background Sync API queues actions while offline, replays on reconnect.

**Sync patterns:**
- Operation log: record each action, replay on reconnect
- Last-Writer-Wins: timestamps/versions, sufficient for most email ops
- CRDTs: overkill for email (mostly append-only) but useful for draft editing

### Search Architecture

| Technology | Strengths | Best For |
|-----------|-----------|---------|
| SQLite FTS5 | Powerful tokenization, stemming, ranking | Desktop clients (Velo reference) |
| MiniSearch | Zero deps, incremental indexing | Web clients, moderate mailboxes |
| Lunr.js | Mature, client-side | Smaller mailboxes |
| Embedding models | Semantic "search by meaning" | AI-powered search |

**Recommended hybrid:** Server-side IMAP SEARCH for large mailboxes + client-side FTS5 for cached messages + embeddings for semantic search.

### Threading

**JWZ Algorithm** (Jamie Zawinski, Netscape Mail, RFC 5256):
1. Build ID table from Message-ID, References, In-Reply-To
2. Link containers into parent-child via References
3. Find roots (no parent = thread root)
4. Group by base subject
5. Sort by date within threads

Gmail uses proprietary X-GM-THRID instead of References-based threading.

### HTML Email Rendering (Hardest Challenge)

**Proven 3-layer security model (Close.com + AdGuard + Velo consensus):**
1. `<iframe srcdoc="..." sandbox="allow-popups allow-same-origin">` — blocks JS
2. CSP: `script-src 'none'` — redundant safety
3. DOMPurify sanitization — clean HTML before rendering
4. Image proxy — route all images through proxy to prevent IP leaks
5. CSS url() interception — parse and rewrite all URL references

### Desktop Framework

**Tauri v2 + Rust** (proven by Velo): native-feeling, lightweight, full SQLite and local AI access. Emerging standard for email clients.

---

## 4. Development Challenges

### Provider-Specific Quirks

| Provider | Connections | Bandwidth | Key Quirks |
|----------|-----------|-----------|------------|
| Gmail | 15 concurrent | 2.5GB/day down, 500MB/day up | OAuth app verification + annual audit. POP discontinued Q1 2026. |
| Outlook | ~8 concurrent | Undocumented | New Outlook removed POP/IMAP. EWS shutdown Oct 2026. |
| Yahoo | 5 per IP | Aggressive limits | Most restrictive. Immediate rate-limiting for misconfigured clients. |

### Push Notifications
- IMAP IDLE: one folder only per connection
- IMAP NOTIFY (RFC 5465): multiple folders, less widely supported
- Web Push (RFC 8030): works even when tab is closed
- JMAP: built-in push support

### Calendar/Contacts
- CalDAV/CardDAV: open standard, works with self-hosted servers
- Google/Microsoft push proprietary APIs over CalDAV/CardDAV
- Google still supports CalDAV/CardDAV with OAuth2
- Microsoft's New Outlook removed CalDAV/CardDAV

---

## 5. Recommended Architecture Stack

Based on all findings:

| Layer | Recommendation | Rationale |
|-------|---------------|-----------|
| Protocol | IMAP + SMTP (OAuth2) baseline. JMAP optional. Gmail API / Graph API as provider-optimized paths. | Covers all providers |
| Storage | SQLite + FTS5 for search | Proven by Velo (33 tables) |
| Web offline | IndexedDB + Service Workers | Standard PWA patterns |
| Real-time | WebSocket to IMAP IDLE. Gmail Pub/Sub. SSE for JMAP. | Sub-second notifications |
| HTML render | Sandboxed iframe + DOMPurify + CSP + image proxy | Close/AdGuard/Velo consensus |
| AI | Ollama (cross-platform) or MLX (macOS). Cloud API fallback. | User plugs in their own |
| Threading | JWZ algorithm | Industry standard |
| Desktop | Tauri v2 + Rust | Proven by Velo, lightweight |
