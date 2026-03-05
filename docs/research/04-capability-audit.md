# Iris — Email Capability Audit (Feature Parity Checklist)

- **Date:** 2026-03-01
- **Status:** Research complete

---

## 1. Core Email Operations

### Compose, Reply, Reply-All, Forward

| Feature | Gmail | Outlook | Apple Mail | Thunderbird | Superhuman | HEY | Spike | Shortwave | Fastmail | ProtonMail | Canary |
|---------|-------|---------|------------|-------------|------------|-----|-------|-----------|----------|------------|--------|
| Compose | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes |
| Reply | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes |
| Reply-All | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes |
| Forward | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes |

### Bounce / Redirect

| Feature | Gmail | Outlook | Apple Mail | Thunderbird | Superhuman | HEY | Spike | Shortwave | Fastmail | ProtonMail | Canary |
|---------|-------|---------|------------|-------------|------------|-----|-------|-----------|----------|------------|--------|
| Bounce/Redirect | No | No | Yes (Redirect) | Yes (built-in since v102) | No | No | No | No | Yes (native) | No | No |

### Rich Text Editing

| Feature | Gmail | Outlook | Apple Mail | Thunderbird | Superhuman | HEY | Spike | Shortwave | Fastmail | ProtonMail | Canary |
|---------|-------|---------|------------|-------------|------------|-----|-------|-----------|----------|------------|--------|
| Bold / Italic | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes |
| Font family/size/color | Yes | Yes | Yes | Yes | No | No/minimal | No/minimal | No/minimal | Yes | Yes | Yes |
| Lists | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes |
| Inline images | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes |
| Tables in compose | No | Yes | Yes | Yes | No | No | No | No | No | No | No |
| Code blocks | No | No | No | Yes (via addon) | No | No | No | Yes (markdown) | No | No | No |
| Markdown compose | No | No | No | No | Yes (native) | No | No | Yes (native) | No | No | No |

### Attachments

| Feature | Gmail | Outlook | Apple Mail | Thunderbird | Superhuman | HEY | Spike | Shortwave | Fastmail | ProtonMail | Canary |
|---------|-------|---------|------------|-------------|------------|-----|-------|-----------|----------|------------|--------|
| Send/Receive | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes |
| Drag-and-drop | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes |
| Inline preview | Yes | Yes | Yes | No | No | No | Yes | Yes | Yes | No | No |
| Large file (cloud) | Yes (Drive, 25MB+) | Yes (OneDrive) | Yes (Mail Drop, 20MB+ via iCloud, 5GB) | Yes (FileLink) | No | No | No | No | No | No | No |
| Size limit | 25MB | 25MB | 20MB direct / 5GB Mail Drop | 25MB | 25MB | 25MB | 25MB | 25MB | 70MB | 25MB | 25MB |

### Draft Management

| Feature | Gmail | Outlook | Apple Mail | Thunderbird | Superhuman | HEY | Spike | Shortwave | Fastmail | ProtonMail | Canary |
|---------|-------|---------|------------|-------------|------------|-----|-------|-----------|----------|------------|--------|
| Auto-save | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes |
| Multiple drafts | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes |
| Scheduled send | Yes | Yes | No | Yes (v115+) | Yes | No | No | Yes | Yes | Yes (paid) | Yes |
| Draft version history | No | No | No | No | No | No | No | No | No | No | No |

### Undo Send

| Client | Undo Send | Details |
|--------|-----------|---------|
| Gmail | Yes | 5–30s configurable |
| Outlook | Yes | 10s web; unreliable recall on desktop |
| Apple Mail | Yes | 10–30s |
| Thunderbird | No | — |
| Superhuman | Yes | 5–30s |
| HEY | No | — |
| Spike | No | — |
| Shortwave | No | — |
| Fastmail | Yes | 30s |
| ProtonMail | Yes | 5–20s |
| Canary | No | — |

### Templates / Canned Responses

| Client | Templates | Details |
|--------|-----------|---------|
| Gmail | Yes | Templates in Advanced settings |
| Outlook | Yes | Quick Parts + My Templates |
| Apple Mail | No | — |
| Thunderbird | No | — |
| Superhuman | Yes | Snippets with variables |
| HEY | No | — |
| Spike | No | — |
| Shortwave | No | — |
| Fastmail | Yes | Native |
| ProtonMail | No | — |
| Canary | Yes | Native |

### Signatures

| Feature | Gmail | Outlook | Apple Mail | Thunderbird | Superhuman | HEY | Spike | Shortwave | Fastmail | ProtonMail | Canary |
|---------|-------|---------|------------|-------------|------------|-----|-------|-----------|----------|------------|--------|
| Multiple signatures | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes |
| Per-account | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes | Yes |
| Auto-switch | No | Yes (rules) | No | No | Yes (per-account) | No | No | No | No | No | No |
| Rich HTML | Yes | Yes | Yes | Yes | Yes | Minimal | Yes | Yes | Yes | Yes | Yes |

---

## 2. Inbox Management

### Inbox Views

| Feature | Gmail | Outlook | Apple Mail | Thunderbird | Superhuman | HEY | Spike | Shortwave | Fastmail | ProtonMail | Canary |
|---------|-------|---------|------------|-------------|------------|-----|-------|-----------|----------|------------|--------|
| Unified inbox | Yes | Yes | Yes | Yes | Yes | No | Yes | No | No | No | Yes |
| Split / multi-pane | No | Yes | No | Yes | Yes | Yes | No | No | No | No | No |
| Priority / Focused | Yes (tabs) | Yes (Focused) | No | No | Yes (VIP+Split) | Yes (Screener+Imbox) | No | Yes (AI bundles) | No | No | Yes |
| Density options | Yes (3 levels) | Yes | Yes | Yes | Yes (compact) | No | No | No | Yes | Yes | No |

### Threading

| Feature | Gmail | Outlook | Apple Mail | Thunderbird | Superhuman | HEY | Spike | Shortwave | Fastmail | ProtonMail | Canary |
|---------|-------|---------|------------|-------------|------------|-----|-------|-----------|----------|------------|--------|
| Conversation view | Yes | Yes | Yes | Yes | Yes | Yes | Yes (chat bubbles) | Yes | Yes | Yes | Yes |
| Toggle thread/flat | Yes | Yes | Yes | Yes | No | No | No | No | Yes | Yes | Yes |

### Labels / Tags

| Client | Labels/Tags | Details |
|--------|-------------|---------|
| Gmail | Yes | Nested, colored, multiple per message |
| Outlook | Yes | Categories, colored |
| Apple Mail | No | — |
| Thunderbird | Yes | Tags, colored |
| Superhuman | No | — |
| HEY | No | — |
| Spike | No | — |
| Shortwave | No | — |
| Fastmail | Yes | Labels, nested, colored |
| ProtonMail | Yes | Labels + folders |
| Canary | No | — |

### Folders

| Client | Folders | Details |
|--------|---------|---------|
| Gmail | Yes | Nested (via labels) |
| Outlook | Yes | Nested |
| Apple Mail | Yes | Nested |
| Thunderbird | Yes | Nested |
| Superhuman | Yes | Nested |
| HEY | No | Different paradigm (no folders) |
| Spike | No | Different paradigm (no folders) |
| Shortwave | Yes | — |
| Fastmail | Yes | Nested |
| ProtonMail | Yes | Nested |
| Canary | Yes | Nested |

### Stars / Flags

| Client | Stars/Flags | Details |
|--------|-------------|---------|
| Gmail | Yes | 12 colored stars |
| Outlook | Yes | Flags with due date |
| Apple Mail | Yes | 7 color flags |
| Thunderbird | Yes | Binary star |
| Superhuman | Yes | Binary star |
| HEY | Yes | Binary star |
| Spike | Yes | Binary star |
| Shortwave | Yes | Binary star |
| Fastmail | Yes | Binary star |
| ProtonMail | Yes | Binary star |
| Canary | Yes | Binary star |

### Archive vs Delete

All 11 clients support both archive and delete. HEY has a different paradigm (no explicit archive).

### Snooze

| Client | Snooze | Notes |
|--------|--------|-------|
| Gmail | Yes | — |
| Outlook | Yes | — |
| Apple Mail | Yes | Ventura+ |
| Thunderbird | No | Via addon |
| Superhuman | Yes | — |
| HEY | No | Uses "Set Aside" (not time-based) |
| Spike | Yes | — |
| Shortwave | Yes | — |
| Fastmail | Yes | — |
| ProtonMail | Yes | — |
| Canary | Yes | — |

### Mute Thread

| Client | Mute Thread |
|--------|-------------|
| Gmail | Yes |
| Outlook | Yes |
| Apple Mail | Yes (Ventura+) |
| Thunderbird | No |
| Superhuman | Yes |
| HEY | No |
| Spike | No |
| Shortwave | Yes |
| Fastmail | No |
| ProtonMail | No |
| Canary | No |

### Spam & Blocking

All 11 clients support report spam and block sender. HEY uses a unique Screener model: approve or reject first-time senders, and the decision persists.

---

## 3. Search & Filtering

### Full-Text Search

| Client | Full-Text | Speed / Notes |
|--------|-----------|---------------|
| Gmail | Yes | Fast (server-side) |
| Outlook | Yes | Fast (server-side) |
| Apple Mail | Yes | Local Spotlight |
| Thunderbird | Yes | Local index (GLODA) |
| Superhuman | Yes | Fastest (pre-cached) |
| HEY | Yes | — |
| Spike | Yes | — |
| Shortwave | Yes | Fast |
| Fastmail | Yes | Server-side |
| ProtonMail | Limited | Metadata only unless encrypted search enabled |
| Canary | Yes | — |

Speed ranking: Superhuman > Gmail > Shortwave > Outlook

### Search Operators

| Client | Operator Support | Details |
|--------|-----------------|---------|
| Gmail | Most complete | from, to, subject, has:attachment, filename, date, size, label, is:unread/starred, AND/OR/NOT, exact phrase |
| Outlook | Extensive | Similar to Gmail |
| Apple Mail | Moderate | Via Spotlight syntax |
| Thunderbird | Moderate | — |
| Superhuman | Extensive | Supports Gmail's operator set |
| HEY | Simple | Basic search |
| Spike | Simple | Basic search |
| Shortwave | Simple | Basic search |
| Fastmail | Extensive | Own query language |
| ProtonMail | Moderate | — |
| Canary | Simple | Basic search |

### Search in Attachment Content

| Client | Search Inside Attachments | Details |
|--------|--------------------------|---------|
| Gmail | Yes | PDFs, Docs, spreadsheets |
| Outlook | Yes | Office files, PDFs |
| Apple Mail | Partial | Via local Spotlight only |
| Thunderbird | No | — |
| Superhuman | No | — |
| HEY | No | — |
| Spike | No | — |
| Shortwave | No | — |
| Fastmail | No | — |
| ProtonMail | No | — |
| Canary | No | — |

### Saved Searches / Smart Folders

| Client | Saved Searches | Details |
|--------|----------------|---------|
| Gmail | No | — |
| Outlook | Yes | Search Folders |
| Apple Mail | Yes | Smart Mailboxes |
| Thunderbird | Yes | Virtual Folders |
| Superhuman | No | — |
| HEY | No | — |
| Spike | No | — |
| Shortwave | No | — |
| Fastmail | Yes | — |
| ProtonMail | No | — |
| Canary | No | — |

### Filters / Rules

| Client | Filters/Rules | Server-Side | Details |
|--------|---------------|-------------|---------|
| Gmail | Yes | Yes | Auto-label, auto-forward, auto-delete, auto-archive |
| Outlook | Yes | Yes | Auto-label, auto-forward, auto-delete, auto-archive |
| Apple Mail | Yes | No (client-side only) | Auto-label, auto-forward, auto-delete, auto-archive |
| Thunderbird | Yes | No (client-side only) | Auto-label, auto-forward, auto-delete, auto-archive |
| Superhuman | No | — | — |
| HEY | No | — | — |
| Spike | No | — | — |
| Shortwave | No | — | — |
| Fastmail | Yes | Yes | Sieve scripting (most powerful) |
| ProtonMail | Yes | Yes | Auto-label, auto-forward, auto-delete, auto-archive |
| Canary | No | — | — |

---

## 4. Organization & Productivity

### Tasks

| Client | Tasks | Details |
|--------|-------|---------|
| Gmail | Yes | Google Tasks |
| Outlook | Yes | Microsoft To Do |
| Apple Mail | Yes | Reminders |
| Thunderbird | Yes | Lightning |
| Superhuman | Yes | Built-in |
| HEY | Yes | Reply Later, Set Aside |
| Spike | Yes | Built-in |
| Shortwave | Yes | To-do label |
| Fastmail | No | — |
| ProtonMail | No | — |
| Canary | No | — |

### Follow-Up Nudges

| Client | Follow-Up Nudges | Details |
|--------|------------------|---------|
| Gmail | Yes | Nudges |
| Outlook | Yes | Flags with dates |
| Apple Mail | Yes | Ventura+ |
| Thunderbird | No | — |
| Superhuman | Yes | "Remind me if no reply" (best implementation) |
| HEY | No | — |
| Spike | No | — |
| Shortwave | Yes | — |
| Fastmail | No | — |
| ProtonMail | No | — |
| Canary | Yes | — |

### Read Receipts

| Client | Read Receipts | Details |
|--------|---------------|---------|
| Gmail | No | — |
| Outlook | Yes | MDN (Message Disposition Notification) |
| Apple Mail | No | — |
| Thunderbird | No | — |
| Superhuman | Yes | Built-in tracking (controversial, now opt-in) |
| HEY | No | Actively blocks tracking |
| Spike | Yes | Spike-to-Spike only |
| Shortwave | No | — |
| Fastmail | No | — |
| ProtonMail | No | — |
| Canary | Yes | Tracking |

### Calendar Integration

| Client | Calendar | Details |
|--------|----------|---------|
| Gmail | Yes | Google Calendar |
| Outlook | Yes | Outlook Calendar + FindTime scheduling |
| Apple Mail | Yes | Calendar.app |
| Thunderbird | Yes | Lightning |
| Superhuman | Yes | Google Calendar sidebar |
| HEY | No | — |
| Spike | Yes | Built-in + scheduling page |
| Shortwave | Yes | Google Calendar |
| Fastmail | Yes | Native CalDAV |
| ProtonMail | Yes | Proton Calendar |
| Canary | Yes | System calendar |

### Contact Management

All clients have address books. Superhuman has the richest contact sidebar (LinkedIn, Twitter, company info). Gmail auto-collects contacts to "Other Contacts."

### Thread Notes

| Client | Thread Notes |
|--------|-------------|
| HEY | Yes ("stickies" on threads) |
| All others | No |

This is a significant gap across the industry.

### Snippets

| Client | Snippets | Details |
|--------|----------|---------|
| Superhuman | Yes | Best implementation: "/" triggered, variables |
| Outlook | Yes | Quick Parts |
| Gmail | Yes | Templates |
| All others | No | — |

---

## 5. Multi-Account & Identity

### Multi-Account Support

| Client | Multi-Account | Supported Providers |
|--------|---------------|---------------------|
| Gmail | Yes | Google only (switch between accounts) |
| Outlook | Yes | Microsoft, Google, Yahoo, IMAP, POP |
| Apple Mail | Yes | Broadest: iCloud, Google, MS, Yahoo, IMAP, POP, Exchange |
| Thunderbird | Yes | IMAP, POP, Exchange (via addon) |
| Superhuman | Yes | Gmail, Outlook |
| HEY | No | Single account only |
| Spike | Yes | Multiple providers |
| Shortwave | Yes | Gmail only (switch between accounts) |
| Fastmail | Yes | Fastmail only |
| ProtonMail | No | Single account + aliases |
| Canary | Yes | Gmail, Outlook, iCloud, IMAP, Exchange |

### Unified Inbox

| Client | Unified Inbox |
|--------|---------------|
| Gmail | No (switch, not unified) |
| Outlook | Yes |
| Apple Mail | Yes |
| Thunderbird | Yes |
| Superhuman | Yes |
| HEY | No (single account) |
| Spike | Yes |
| Shortwave | No (switch) |
| Fastmail | No |
| ProtonMail | No |
| Canary | Yes |

### Send As / Alias

Most clients support Send As / Alias functionality. Fastmail has the most powerful alias system (unlimited aliases, per-alias signatures). ProtonMail integrates with SimpleLogin for disposable aliases.

### Delegation

| Client | Delegation | Details |
|--------|------------|---------|
| Gmail | Yes | Workspace delegate access |
| Outlook | Yes | Send on Behalf, shared mailbox |
| Apple Mail | No | — |
| Thunderbird | No | — |
| Superhuman | No | — |
| HEY | Yes | HEY for Work |
| Spike | Yes | Team channels |
| Shortwave | Yes | Shared channels |
| Fastmail | No | — |
| ProtonMail | No | — |
| Canary | No | — |

---

## 6. Notifications & Sync

### Push Notifications

All modern clients support desktop and mobile push notifications.

### Notification Customization

| Feature | Gmail | Outlook | Apple Mail | Thunderbird | Superhuman | HEY | Spike | Shortwave | Fastmail | ProtonMail | Canary |
|---------|-------|---------|------------|-------------|------------|-----|-------|-----------|----------|------------|--------|
| Per-account | No | Yes | Yes | Yes | Yes | No | Yes | No | Yes | No | Yes |
| Per-sender / VIP | No | Yes | Yes | No | Yes | Yes | No | No | No | No | No |
| DND schedule | No | Yes | No | No | Yes | No | Yes | No | No | No | Yes |

### Offline Support

| Client | Offline | Details |
|--------|---------|---------|
| Gmail | Yes | PWA |
| Outlook | Yes | Desktop app |
| Apple Mail | Yes | Native (full local copies) |
| Thunderbird | Yes | Local copies |
| Superhuman | Yes | Desktop app |
| HEY | Yes | PWA + mobile |
| Spike | Partial | Mobile |
| Shortwave | Partial | Web-only; degrades significantly |
| Fastmail | Partial | Web-only; degrades significantly |
| ProtonMail | Partial | Mobile |
| Canary | Yes | Native mobile |

---

## 7. Security & Privacy

### End-to-End Encryption

| Client | E2E Encryption | Details |
|--------|----------------|---------|
| Gmail | No | TLS only |
| Outlook | Yes | S/MIME |
| Apple Mail | Yes | S/MIME |
| Thunderbird | Yes | OpenPGP built-in + S/MIME |
| Superhuman | No | — |
| HEY | No | — |
| Spike | No | — |
| Shortwave | No | — |
| Fastmail | No | — |
| ProtonMail | Yes | Default for Proton-to-Proton |
| Canary | Yes | PGP built-in |

### Tracking Pixel Blocking

| Client | Tracking Protection | Details |
|--------|---------------------|---------|
| Gmail | Partial | Proxies images (hides IP but confirms open) |
| Outlook | No | — |
| Apple Mail | Yes | Mail Privacy Protection (most comprehensive, prefetches via proxy) |
| Thunderbird | Partial | Blocks remote images by default |
| Superhuman | No | — |
| HEY | Yes | Spy Pixel report (tells you who tracks you) |
| Spike | No | — |
| Shortwave | No | — |
| Fastmail | No | — |
| ProtonMail | Yes | Proxy |
| Canary | Yes | Detection + blocking |

### Link Safety

| Client | Link Safety | Details |
|--------|-------------|---------|
| Gmail | Yes | Warnings + blocks known malicious URLs |
| Outlook | Yes | SafeLinks (real-time URL scanning) |
| All others | Minimal | — |

### Password-Protected Emails

| Client | Password-Protected | Details |
|--------|-------------------|---------|
| Gmail | Yes | Confidential Mode + SMS passcode |
| Outlook | Yes | Encryption with link |
| ProtonMail | Yes | Password-encrypted to non-Proton recipients |
| All others | No | — |

### Expiring Messages

| Client | Expiring Messages | Details |
|--------|-------------------|---------|
| Gmail | Yes | Confidential Mode (1 day to 5 years) |
| Outlook | Yes | M365 encryption |
| ProtonMail | Yes | Expiring for external recipients |
| All others | No | — |

### Privacy Report

| Client | Privacy Report | Details |
|--------|----------------|---------|
| HEY | Yes | Spy Pixel report per sender |
| Apple Mail | Yes | Privacy Protection report |
| Canary | Yes | Tracking report |
| All others | No | — |

---

## 8. Customization & Settings

### Theme

| Client | Theme | Details |
|--------|-------|---------|
| Gmail | Yes | Light, dark, background images |
| Outlook | Yes | Light, dark |
| Apple Mail | Yes | Light, dark (follows system) |
| Thunderbird | Yes | Light, dark, installable themes |
| Superhuman | Yes | Light, dark |
| HEY | Yes | Light, dark |
| Spike | Yes | Light, dark |
| Shortwave | Yes | Light, dark |
| Fastmail | Yes | Light, dark |
| ProtonMail | Yes | Light, dark |
| Canary | Yes | Light, dark |

### Layout Flexibility

| Client | Layout | Details |
|--------|--------|---------|
| Gmail | High | Density options, pane positions, multiple inboxes |
| Outlook | High | Focused toggle, reading pane, board view |
| Apple Mail | Moderate | — |
| Thunderbird | Highest | 3-pane, 2-pane, vertical, horizontal, cards |
| Superhuman | Moderate | — |
| HEY | Low | Most opinionated / fixed layout |
| Spike | Low | — |
| Shortwave | Moderate | — |
| Fastmail | Moderate | — |
| ProtonMail | Moderate | — |
| Canary | Moderate | — |

### Keyboard Shortcuts

| Client | Keyboard Shortcuts | Details |
|--------|-------------------|---------|
| Gmail | Yes | Extensive but not customizable |
| Outlook | Yes | Standard set |
| Apple Mail | Yes | Standard set |
| Thunderbird | Yes | Customizable via addon |
| Superhuman | Yes | Most comprehensive, customizable, Vim-inspired |
| HEY | Yes | Standard set |
| Spike | Yes | Standard set |
| Shortwave | Yes | Standard set |
| Fastmail | Yes | Standard set |
| ProtonMail | Yes | Standard set |
| Canary | Yes | Standard set |

### Swipe Actions (Mobile)

All mobile clients support swipe actions. Most are customizable.

### Accessibility

| Client | Accessibility | Details |
|--------|---------------|---------|
| Outlook | Strongest | ARIA, Accessibility Checker, Immersive Reader |
| Apple Mail | Strong | VoiceOver |
| Thunderbird | Good | Themes, font sizing |
| All others | Standard | — |

---

## 9. Collaboration

### Shared Inbox

| Client | Shared Inbox | Details |
|--------|-------------|---------|
| Gmail | Yes | Workspace (Collaborative Inbox) |
| Outlook | Yes | Shared Mailbox |
| Superhuman | Yes | Superhuman for Teams |
| HEY | Yes | HEY for Work |
| Spike | Yes | Team channels |
| Shortwave | Yes | Shared channels |
| Apple Mail | No | — |
| Thunderbird | No | — |
| Fastmail | No | — |
| ProtonMail | No | — |
| Canary | No | — |

### Internal Comments (Invisible to External Recipients)

| Client | Internal Comments |
|--------|-------------------|
| Superhuman | Yes |
| HEY | Yes (HEY for Work) |
| Spike | Yes |
| Shortwave | Yes |
| Gmail | No |
| Outlook | No |
| Apple Mail | No |
| Thunderbird | No |
| Fastmail | No |
| ProtonMail | No |
| Canary | No |

### Collision Detection ("Someone Else Is Replying")

| Client | Collision Detection |
|--------|---------------------|
| Superhuman | Yes |
| HEY | Yes (HEY for Work) |
| Shortwave | Yes |
| Front | Yes |
| All others | No |

This is a very rare feature across the industry.

### Shared Drafts

| Client | Shared Drafts |
|--------|---------------|
| Gmail | Yes (Workspace Collaborative Inbox) |
| Shortwave | Yes |
| All others | No |

---

## 10. Integrations & Extensibility

### Calendar Integration

| Client | Calendar | Protocol/Service |
|--------|----------|-----------------|
| Gmail | Yes | Google Calendar |
| Outlook | Yes | Outlook Calendar |
| Apple Mail | Yes | CalDAV (Calendar.app) |
| Thunderbird | Yes | CalDAV (Lightning) |
| Superhuman | Yes | Google Calendar |
| HEY | No | — |
| Spike | Yes | Google Calendar |
| Shortwave | Yes | Google Calendar |
| Fastmail | Yes | Native CalDAV |
| ProtonMail | Yes | Proton Calendar |
| Canary | Yes | CalDAV (system calendar) |

### Cloud Storage

| Client | Cloud Storage | Details |
|--------|---------------|---------|
| Gmail | Yes | Google Drive |
| Outlook | Yes | OneDrive |
| Apple Mail | Yes | iCloud Mail Drop |
| Thunderbird | Yes | FileLink (Dropbox, WeTransfer, custom) |
| Superhuman | No | — |
| HEY | No | — |
| Spike | Yes | Drive, Dropbox, OneDrive |
| Shortwave | No | — |
| Fastmail | No | — |
| ProtonMail | No | — |
| Canary | No | — |

### Plugin / Extension System

| Client | Plugin System | Details |
|--------|---------------|---------|
| Gmail | Yes | Workspace Add-ons |
| Outlook | Yes | Office Add-ins |
| Apple Mail | Yes | Mail Extensions |
| Thunderbird | Yes | Most mature addon ecosystem |
| Superhuman | No | — |
| HEY | No | — |
| Spike | No | — |
| Shortwave | No | — |
| Fastmail | No | — |
| ProtonMail | No | — |
| Canary | No | — |

### Public API

| Client | Public API | Details |
|--------|-----------|---------|
| Gmail | Yes | Gmail API (REST) |
| Outlook | Yes | Microsoft Graph API |
| Fastmail | Yes | JMAP (open standard, most elegant) |
| Apple Mail | No | — |
| Superhuman | No | — |
| HEY | No | — |
| Spike | No | — |
| Shortwave | No | — |
| ProtonMail | No | — |
| Canary | No | — |
| Thunderbird | No | — |

### Webhooks

| Client | Webhooks | Details |
|--------|----------|---------|
| Gmail | Yes | Pub/Sub |
| Outlook | Yes | Graph subscriptions |
| Fastmail | Yes | JMAP push |
| All others | No | — |

### Automation (Zapier, Make, Power Automate, IFTTT)

| Client | Automation | Details |
|--------|-----------|---------|
| Gmail | Yes | Zapier, Make, Power Automate, IFTTT |
| Outlook | Yes | Zapier, Make, Power Automate, IFTTT |
| All others | Very limited | — |

---

## 11. Newsletter / Subscription Management

### One-Click Unsubscribe

| Client | One-Click Unsubscribe |
|--------|----------------------|
| Gmail | Yes (prominent link surfaced) |
| Outlook | Yes |
| Apple Mail | Yes (Ventura+) |
| Superhuman | Yes |
| HEY | Yes |
| Spike | No |
| Shortwave | Yes |
| Fastmail | Yes |
| ProtonMail | Yes |
| Canary | Yes |
| Thunderbird | No |

### Newsletter Feed View

| Client | Feed View | Details |
|--------|-----------|---------|
| HEY | Yes | "The Feed" — scrollable inline view |
| Shortwave | Yes | AI-powered bundles |
| All others | No | — |

This is a major gap across the industry.

### Subscription Dashboard / Bulk Management

No client offers comprehensive subscription management tools. HEY Screener is the closest approximation. Third-party tools (Unroll.me, Clean Email) fill this gap.

---

## 12. Advanced / Power User

### Mail Merge

| Client | Mail Merge | Details |
|--------|-----------|---------|
| Gmail | Yes | Workspace multi-send (up to 1500 recipients) |
| Outlook | Yes | Desktop (Word merge) |
| All others | No | — |

### Follow-Up If No Reply

| Client | Follow-Up | Details |
|--------|-----------|---------|
| Gmail | Yes | Nudges (passive) |
| Outlook | Yes | Flag with date |
| Apple Mail | Yes | Ventura+ |
| Superhuman | Yes | Best: configurable "remind if no reply" |
| Shortwave | Yes | — |
| Canary | Yes | — |
| Thunderbird | No | — |
| HEY | No | — |
| Spike | No | — |
| Fastmail | No | — |
| ProtonMail | No | — |

### Analytics

| Client | Analytics | Details |
|--------|-----------|---------|
| Superhuman | Yes | Response time stats, open tracking |
| Canary | Yes | Open/link tracking |
| All others | No | — |

### Import / Export

| Client | Import/Export | Details |
|--------|-------------|---------|
| Gmail | Yes | Google Takeout |
| Thunderbird | Yes | mbox, EML, PST (via addon) |
| ProtonMail | Yes | Import-Export app (mbox, EML, PST) |
| Fastmail | Yes | Migration tool |
| All others | Limited | — |

### Custom Domain

| Client | Custom Domain |
|--------|--------------|
| Gmail | Yes (Workspace) |
| Outlook | Yes (M365) |
| Fastmail | Yes (excellent) |
| ProtonMail | Yes |
| HEY | Yes (HEY for Domains) |
| Apple Mail | No |
| Thunderbird | No (client only) |
| Superhuman | No |
| Spike | No |
| Shortwave | No |
| Canary | No |

### Raw Message Source

| Client | View Raw Source |
|--------|----------------|
| Gmail | Yes |
| Outlook | Yes |
| Apple Mail | Yes |
| Thunderbird | Yes |
| Fastmail | Yes |
| ProtonMail | Yes |
| Canary | Yes |
| Superhuman | No |
| HEY | No |
| Spike | No |
| Shortwave | No |

---

## 13. Mobile-Specific

### Apple Watch

| Client | Apple Watch |
|--------|-------------|
| Outlook | Yes |
| Apple Mail | Yes (deepest integration) |
| Spike | Yes |
| Canary | Yes |
| Gmail | No (removed 2017) |
| Superhuman | No |
| HEY | No |
| Thunderbird | No |
| Shortwave | No |
| Fastmail | No |
| ProtonMail | No |

### Widgets (iOS / Android)

| Client | Widgets |
|--------|---------|
| Gmail | Yes (both platforms) |
| Outlook | Yes (both platforms) |
| Apple Mail | Yes (iOS) |
| Spike | Yes |
| Canary | Yes |
| Superhuman | No |
| HEY | No |
| Thunderbird | No |
| Shortwave | No |
| Fastmail | No |
| ProtonMail | No |

### Actionable Notifications

| Client | Actionable Notifications | Details |
|--------|--------------------------|---------|
| Gmail | Yes | Archive, reply, mark read from notification |
| Outlook | Yes | Archive, reply, mark read from notification |
| Apple Mail | Yes | Archive, reply, mark read from notification |
| Superhuman | Yes | Archive, reply, mark read from notification |
| Spike | Yes | Archive, reply, mark read from notification |
| ProtonMail | Yes | Archive, reply, mark read from notification |
| Canary | Yes | Archive, reply, mark read from notification |
| HEY | No | — |
| Thunderbird | No | — |
| Shortwave | No | — |
| Fastmail | No | — |

---

## 14. AI Features — Current State

### Smart Compose

| Client | Smart Compose |
|--------|---------------|
| Gmail | Yes |
| Outlook | Yes |
| Superhuman | Yes |
| Spike | Yes |
| Shortwave | Yes |
| Canary | Yes |
| Apple Mail | No |
| Thunderbird | No |
| HEY | No |
| Fastmail | No |
| ProtonMail | No |

### AI Replies

| Client | AI Replies | Details |
|--------|-----------|---------|
| Gmail | Yes | Help Me Write |
| Outlook | Yes | Copilot |
| Apple Mail | Yes | Apple Intelligence |
| Superhuman | Yes | — |
| Spike | Yes | — |
| Shortwave | Yes | — |
| Canary | Yes | — |
| HEY | No | — |
| Thunderbird | No | — |
| Fastmail | No | — |
| ProtonMail | No | — |

### Thread Summarization

| Client | Summarization | Details |
|--------|---------------|---------|
| Gmail | Yes | Gemini |
| Outlook | Yes | Copilot |
| Apple Mail | Yes | Apple Intelligence |
| Superhuman | Yes | Auto at top of thread |
| Shortwave | Yes | — |
| Spike | Yes | — |
| Canary | Yes | — |
| HEY | No | — |
| Thunderbird | No | — |
| Fastmail | No | — |
| ProtonMail | No | — |

### Priority / Smart Categorization

| Client | Smart Categorization | Details |
|--------|---------------------|---------|
| Gmail | Yes | Tabs (Primary, Social, Promotions, Updates) |
| Outlook | Yes | Focused Inbox |
| Superhuman | Yes | VIP + Split Inbox |
| HEY | Yes | Screener-based |
| Shortwave | Yes | AI bundles (most granular) |
| Spike | Yes | — |
| Canary | Yes | — |
| Apple Mail | No | — |
| Thunderbird | No | — |
| Fastmail | No | — |
| ProtonMail | No | — |

### Natural Language Search

| Client | NL Search | Details |
|--------|-----------|---------|
| Gmail | Yes | Gemini |
| Outlook | Yes | Copilot |
| Shortwave | Yes | — |
| All others | No | — |

### Writing Style Learning

| Client | Style Learning | Details |
|--------|---------------|---------|
| Gmail | Yes | Smart Compose adapts over time |
| Superhuman | Yes | Adapts to tone |
| Outlook | Limited | — |
| All others | No | — |

### Task Extraction

| Client | Task Extraction | Details |
|--------|----------------|---------|
| Gmail | Yes | Gemini |
| Outlook | Yes | Copilot |
| Apple Mail | Yes | Apple Intelligence |
| Shortwave | Yes | — |
| All others | No | — |

### Contact Intelligence

| Client | Contact Intelligence | Details |
|--------|---------------------|---------|
| Superhuman | Yes | Social profiles, company info |
| Outlook | Yes | LinkedIn integration |
| All others | No | — |

---

## Summary — Key Gaps (Innovation Opportunities)

The following features represent areas where no client (or very few clients) delivers a strong solution. These are the highest-leverage opportunities for Iris to differentiate:

| # | Gap | Current State |
|---|-----|---------------|
| 1 | Draft version history | Zero clients offer this |
| 2 | Thread-level private notes | Only HEY |
| 3 | Collision detection ("someone else is replying") | Only Superhuman, HEY, Shortwave (team editions only) |
| 4 | Newsletter feed view | Only HEY, Shortwave |
| 5 | Privacy-preserving AI | Nobody (Apple partial with on-device processing) |
| 6 | Subscription management dashboard | Nobody |
| 7 | Cross-account semantic search | Nobody |
| 8 | Open agent/plugin connectivity | Nobody |
| 9 | Messaging + email view toggle | Spike tried but forced; no elegant toggle exists |
| 10 | Offline-first with full parity | Most web clients degrade significantly |
| 11 | Search in attachment content | Only Gmail, Outlook (server-side) |
| 12 | Bounce/redirect on web | Missing from most web clients |
