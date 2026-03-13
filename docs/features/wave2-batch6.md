# Wave 2 Batch 6: Feature Documentation

## #37 Draft Version History

Tracks every meaningful change to a draft as a versioned snapshot, allowing users to view past states, compare revisions, and restore any prior version.

### API

**POST** `/api/drafts/{draft_id}/versions`

Manually saves the current state of a draft as a new version.

**Request:**
```json
{
  "account_id": "acc-abc123"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `account_id` | string | yes | Account that owns the draft |

**Response:**
```json
{
  "id": 14,
  "draft_id": "draft-abc123",
  "account_id": "acc-abc123",
  "version_number": 3,
  "subject": "Q3 Planning",
  "body": "Hi team,\n\nHere is the revised plan...",
  "to_addresses": "[\"alice@example.com\"]",
  "cc_addresses": null,
  "word_count": 47,
  "created_at": 1710288000
}
```

Returns `404` if the draft does not exist or does not belong to the account.

---

**GET** `/api/drafts/{draft_id}/versions`

Lists all versions for a draft, newest first. Returns metadata only (no body).

**Response:**
```json
[
  {
    "id": 14,
    "draft_id": "draft-abc123",
    "version_number": 3,
    "subject": "Q3 Planning",
    "word_count": 47,
    "created_at": 1710288000
  },
  {
    "id": 9,
    "draft_id": "draft-abc123",
    "version_number": 2,
    "subject": "Q3 Planning",
    "word_count": 32,
    "created_at": 1710284400
  }
]
```

---

**GET** `/api/drafts/{draft_id}/versions/{version_number}`

Fetches the full content of a specific version including body.

**Response:** Same shape as `VersionDetail` (see save_version response above).

Returns `404` if the version does not exist.

---

**GET** `/api/drafts/{draft_id}/versions/diff?from={n}&to={m}`

Computes a line-level diff between two versions of the same draft.

| Param | Type | Description |
|-------|------|-------------|
| `from` | int | Source version number |
| `to` | int | Target version number |

**Response:**
```json
{
  "from_version": 1,
  "to_version": 3,
  "from_word_count": 18,
  "to_word_count": 47,
  "words_added": 31,
  "words_removed": 2,
  "lines": [
    { "kind": "unchanged", "text": "Hi team," },
    { "kind": "removed",   "text": "Here is the old plan." },
    { "kind": "added",     "text": "Here is the revised plan..." }
  ]
}
```

`kind` values: `"added"`, `"removed"`, `"unchanged"`.

---

**POST** `/api/drafts/{draft_id}/versions/{version_number}/restore`

Overwrites the live draft with the content from the specified version, then records the restored state as a new version entry.

**Response:** The newly created `VersionDetail` representing the restored snapshot.

Returns `404` if the version or draft is not found, or if the draft is not owned by the account that saved the version.

### How It Works

Version storage is backed by the `draft_versions` table (migration 031) with a `UNIQUE(draft_id, version_number)` constraint to prevent duplicates.

The `auto_version_if_changed` helper is called by the draft update handler every time the draft body is saved. It compares the incoming body against the latest stored version and only writes a new row when the body has changed — identical saves are skipped as no-ops. The first save to a draft always creates version 1.

Diff computation uses a longest common subsequence (LCS) algorithm operating at the line level. The LCS table is built in O(m×n) time, then traced back to emit `added`, `removed`, and `unchanged` entries. Word counts in the diff response are computed from the `added`/`removed` lines only.

Restore overwrites the `messages` row (subject, body_text, to_addresses, cc_addresses, snippet, updated_at) and appends the restored state as a new version, so the full history is preserved.

### Configuration

No configuration required. Auto-versioning is always active when AI is off — it is a pure database operation with no AI dependency.

---

## #57 Multi-Language Translate

AI-powered translation for arbitrary text and complete email messages, with language auto-detection, formality control, and context-aware tone selection.

### API

**POST** `/api/ai/translate`

Translates a block of text to a target language.

**Request:**
```json
{
  "text": "Let's discuss the roadmap for next quarter.",
  "source_language": "English",
  "target_language": "Spanish",
  "context": "business",
  "formality": "formal"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `text` | string | yes | Text to translate (max 50KB) |
| `target_language` | string | yes | Target language name (e.g. `"Japanese"`) |
| `source_language` | string | no | Source language; auto-detected if omitted |
| `context` | string | no | `"email_compose"` (default), `"casual"`, `"business"` |
| `formality` | string | no | `"formal"` (default), `"informal"`, `"neutral"` |

**Response:**
```json
{
  "translated_text": "Discutamos la hoja de ruta del próximo trimestre.",
  "detected_source": "English",
  "target_language": "Spanish"
}
```

Returns `400` for invalid `context` or `formality` values, `413` if text exceeds 50KB, `503` if AI is disabled.

---

**POST** `/api/ai/detect-language`

Identifies the language of a text sample.

**Request:**
```json
{
  "text": "Bonjour, comment allez-vous?"
}
```

**Response:**
```json
{
  "language": "French",
  "confidence": 0.98
}
```

`confidence` is a 0–1 float, clamped from the AI response. Returns `503` if AI is disabled.

---

**POST** `/api/ai/translate-email`

Translates a full email (subject + body) as a single operation, preserving email structure.

**Request:**
```json
{
  "subject": "Project Update",
  "body": "Hi team,\n\nPlease see the attached report.",
  "target_language": "German",
  "formality": "formal"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `subject` | string | yes | Email subject line |
| `body` | string | yes | Email body (must be non-empty) |
| `target_language` | string | yes | Target language name |
| `formality` | string | no | `"formal"` (default), `"informal"`, `"neutral"` |

**Response:**
```json
{
  "subject": "Projektaktualisierung",
  "body": "Liebes Team,\n\nBitte lesen Sie den beigefügten Bericht.",
  "detected_source": "English",
  "target_language": "German"
}
```

Returns `413` if combined subject + body length exceeds 50KB.

### How It Works

All three endpoints delegate to the configured AI provider via `ProviderPool::generate`. The AI is instructed to return a strict JSON response (no markdown). A JSON extraction helper strips common markdown fence wrappers (`\`\`\`json ... \`\`\``) before parsing, so models that wrap their output are handled gracefully.

System prompts are constructed dynamically:
- `translate`: combines context description ("professional email composition", "casual conversation", "formal business communication") with formality instruction, instructs preservation of paragraph breaks and sign-offs.
- `translate-email`: instructs the model to translate both fields together, preserving email structure.
- `detect-language`: fixed system prompt asking for language name plus a 0–1 confidence float.

Missing optional fields in the AI response are defaulted: `detected_source` → `"Unknown"`, `confidence` → `1.0`.

### Configuration

Requires AI enabled (`ai_enabled = true` in config) and at least one configured AI provider. No per-language configuration.

---

## #61 Relationship Intelligence

Provides detailed, statistically-rich contact profiles derived from email history: interaction counts, response time, communication patterns, topic extraction, and an optional AI-generated narrative summary.

### API

**GET** `/api/contacts/intelligence/summary?limit={n}`

Returns the top contacts ranked by relationship score.

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `limit` | int | 10 | Max contacts to return (capped at 50) |

**Response:**
```json
{
  "contacts": [
    {
      "email": "alice@example.com",
      "display_name": "Alice Smith",
      "score": 0.82,
      "total_emails": 47,
      "last_contact": 1710288000
    }
  ],
  "total_contacts": 12
}
```

---

**GET** `/api/contacts/{email}/intelligence`

Returns the full intelligence profile for a specific contact.

**Response:**
```json
{
  "email": "alice@example.com",
  "display_name": "Alice Smith",
  "relationship_score": 0.82,
  "score_breakdown": {
    "frequency": 0.9,
    "recency": 0.75,
    "reply_rate": 0.85,
    "bidirectional": 1.0,
    "thread_depth": 0.6
  },
  "stats": {
    "total_emails": 47,
    "sent_by_you": 22,
    "received": 25,
    "avg_response_time_hours": 3.4,
    "first_contact": 1680000000,
    "last_contact": 1710288000
  },
  "common_topics": ["Budget", "Planning", "Q3", "Review", "Sync"],
  "communication_pattern": {
    "most_active_day": "Tuesday",
    "most_active_hour": 10,
    "avg_emails_per_week": 2.8
  }
}
```

Returns `404` if no email history exists for the contact.

---

**POST** `/api/contacts/{email}/intelligence/ai-summary`

Generates an AI-written relationship narrative for a contact.

**Response:**
```json
{
  "summary": "Alice is a close collaborator you've worked with regularly since early 2023, primarily on budget planning and Q3 reviews. Communication is highly bidirectional with a fast average response time.",
  "key_insights": [
    "Strong bidirectional communication pattern",
    "Primarily discusses budget and planning topics",
    "Quick responder with ~3.4 hour average reply time"
  ]
}
```

### How It Works

The `get_contact_intelligence` handler computes all statistics live from the `messages` table using a series of targeted SQL queries:

1. **Email counts**: separate queries for received (`from_address = contact`) and sent (user's Sent folder with contact in `to_addresses` LIKE search, LIKE metacharacters escaped).
2. **Response time**: for each thread where the contact sent a message, finds the earliest user reply after that message; averages all measured deltas.
3. **Communication patterns**: SQLite `strftime('%w')` for day-of-week and `strftime('%H')` for hour, both aggregated with `COUNT(*) GROUP BY` to find the mode.
4. **Common topics**: fetches up to 200 distinct subjects, splits on non-alphanumeric characters, lowercases, filters a 70-word stop list, counts occurrences, returns top 5 title-cased words.
5. **Relationship score**: read directly from the `relationship_scores` table if a prior compute exists; defaults to 0.0 otherwise.

The AI summary endpoint builds a structured prompt from the computed stats and instructs the model to respond with a `SUMMARY:` line followed by `INSIGHTS:` bullet lines. A parser extracts these sections, stripping bullet markers (`•`, `-`, `*`, `–`). If parsing fails, the first three lines of the response are used as a fallback summary.

Email comparison is always case-insensitive via `LOWER()` in all SQL queries.

### Configuration

No configuration required for statistical endpoints. AI summary requires AI enabled.

---

## #74 Link Safety Scanner

Analyzes every URL in an email's HTML body for phishing indicators without any AI or external network requests: lookalike domains, homoglyph substitution, URL shorteners, suspicious paths, and display text mismatches.

### API

**POST** `/api/messages/{id}/scan-links`

Scans all links in the specified message and returns per-link safety analysis plus a summary.

**Response:**
```json
{
  "links": [
    {
      "url": "https://docs.google.com/spreadsheet/abc",
      "display_text": "Q3 Report",
      "safety_level": "safe",
      "reasons": ["HTTPS", "Known trusted domain", "Verified Google domain"],
      "domain": "docs.google.com",
      "is_shortened": false,
      "redirect_target": null
    },
    {
      "url": "https://paypa1-secure.com/verify",
      "display_text": "Verify your account",
      "safety_level": "danger",
      "reasons": ["HTTPS", "Lookalike domain of paypal.com", "Suspicious path: /verify"],
      "domain": "paypa1-secure.com",
      "is_shortened": false,
      "redirect_target": null
    },
    {
      "url": "https://bit.ly/3xK9mP2",
      "display_text": "Click here",
      "safety_level": "caution",
      "reasons": ["HTTPS", "URL shortener", "Destination unknown"],
      "domain": "bit.ly",
      "is_shortened": true,
      "redirect_target": null
    }
  ],
  "summary": {
    "total": 3,
    "safe": 1,
    "caution": 1,
    "danger": 1
  }
}
```

Returns `404` if the message does not exist.

### Safety Levels

| Level | Meaning |
|-------|---------|
| `safe` | HTTPS + known trusted domain, no red flags |
| `caution` | Unknown domain, HTTP-only, URL shortener, or suspicious path |
| `danger` | Lookalike domain, homoglyph substitution, or display text mismatch |

### Detection Signals

| Signal | Level | Description |
|--------|-------|-------------|
| Known trusted domain | safe | Matches a hardcoded list of ~50 major services (Google, Microsoft, Apple, Amazon, Slack, GitHub, etc.) including subdomains |
| HTTPS | safe indicator | Presence adds safe reason; absence adds caution signal |
| URL shortener | caution | Domain matches 18 known shorteners (bit.ly, t.co, tinyurl.com, etc.) |
| Suspicious path | caution | Path starts with `/verify`, `/login`, `/secure`, `/account`, `/password`, `/signin`, `/update`, `/confirm`, `/validate`, `/reset`, `/auth`, `/authenticate` — only on untrusted domains |
| Lookalike domain | danger | Levenshtein distance ≤ 2 between the second-level domain label and a known brand label (minimum 5-char labels to avoid false positives); also checks hyphenated brand prefix (e.g. `paypal-secure.com`) |
| Homoglyph substitution | danger | Common character substitutions normalized (`0→o`, `1→l`, `rn→m`, `vv→w`, `cl→d`); flags if normalized domain matches a known brand |
| Display text mismatch | danger | Display text contains a recognizable domain that differs from the actual URL domain |

### How It Works

Link extraction runs in two passes over the raw HTML:

1. **Anchor pass**: scans for `<a href="...">` tags, extracts the `href` (http/https only, skips mailto/javascript), strips inner HTML tags to get display text.
2. **Plain-text pass**: scans for bare `http://` or `https://` sequences not inside HTML tags, trims trailing punctuation. Deduplicates against anchors already found.

Each extracted link is analyzed independently. The safety level is computed from signal counts: any danger signal overrides; two or more caution signals, or one caution signal on an untrusted domain, yields caution; trusted + HTTPS with no signals yields safe.

`redirect_target` is always `null` — no live network requests are made.

### Configuration

No configuration. No AI required. Pure static analysis.

---

## #76 Privacy Report

Scans email history for tracking pixels and link trackers, identifying which senders use them and how frequently, with trend comparison across time periods.

### API

**GET** `/api/privacy/report?days={n}&account_id={id}`

Generates a privacy report for the specified time window.

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `days` | int | 30 | Lookback period in days (clamped 1–365) |
| `account_id` | string | all | Filter to a single account |

**Response:**
```json
{
  "period_days": 30,
  "summary": {
    "total_emails": 312,
    "emails_with_trackers": 87,
    "tracker_percentage": 27.88,
    "unique_trackers": 5,
    "pixels_blocked": 87
  },
  "trackers": [
    {
      "domain": "mcimg.com",
      "tracker_name": "Mailchimp",
      "count": 34,
      "tracker_type": "open_pixel",
      "first_seen": 1708300000,
      "last_seen": 1710200000,
      "senders": ["newsletter@example.com", "updates@shop.com"]
    }
  ],
  "top_senders_with_trackers": [
    {
      "sender": "newsletter@example.com",
      "sender_name": "Example Newsletter",
      "tracker_count": 18,
      "tracker_domains": ["mcimg.com"]
    }
  ],
  "trend": {
    "current_period": 27.88,
    "previous_period": 31.4,
    "direction": "improving"
  }
}
```

`trend.direction` values: `"improving"` (current % decreased by >0.5pp), `"worsening"` (increased by >0.5pp), `"stable"`.

---

**GET** `/api/privacy/trackers?account_id={id}`

Returns all-time tracker domain totals found across all emails (no time window).

**Response:**
```json
{
  "trackers": [
    {
      "domain": "sendgrid.net",
      "name": "SendGrid",
      "type": "open_pixel",
      "total_occurrences": 142
    },
    {
      "domain": "t.co",
      "name": "Twitter/X",
      "type": "link_track",
      "total_occurrences": 23
    }
  ]
}
```

### Tracker Types

| Type | Meaning |
|------|---------|
| `open_pixel` | 1×1 tracking image that fires on email open |
| `link_track` | Click-tracking redirect URL |

### Known Tracker Database

The tracker database is a static embedded list covering 20+ vendors:

| Vendor | Domains |
|--------|---------|
| Mailchimp | `mcimg.com`, `list-manage.com`, `mailchimp.com` |
| HubSpot | `t.hubspotemail.net`, `track.hubspot.com`, `hubspot.com`, `hsforms.com` |
| SendGrid | `sendgrid.net`, `sendgrid.com` |
| Mailgun | `mailgun.org` |
| Amazon SES | `amazonses.com` |
| Google Analytics | `google-analytics.com` |
| Facebook Pixel | `facebook.com`, `connect.facebook.net` |
| Mailtrack | `mailtrack.io` |
| ConvertKit | `convertkit.com` |
| Drip | `drip.com` |
| Pardot | `pardot.com` |
| Marketo | `mktotracking.com` |
| Others | Beehiiv, Sailthru, Salesforce Marketing Cloud, Twitter/X |

Unknown domains are classified by subdomain prefix heuristics: `click.*` / `clicks.*` → Generic Click Tracker (`link_track`); `open.*` / `opens.*` → Generic Open Tracker (`open_pixel`); `track.*` / `tracking.*` / `trk.*` / `t.*` → Generic Tracker (`open_pixel`).

### How It Works

Tracker detection relies on the `has_tracking_pixels` flag set during email sync (migration 033 adds this column to `messages`). The privacy report queries only messages where this flag is true, then re-scans their HTML bodies to extract tracker domains.

HTML scanning runs two passes: first, all `<img>` tags are inspected — a tag is flagged as a tracker if it has a 1×1 pixel dimension (`width ≤ 1` and `height ≤ 1`) or if its `src` domain matches the tracker list. Second, all `src=` and `href=` attribute values are scanned for known tracker domains to catch link trackers in anchors.

Per-domain statistics (count, first/last seen, sender set) and per-sender statistics are computed in-memory from the scanned rows. The trend is computed by running the same tracker-percentage calculation over the immediately preceding equal-length period.

The `list_trackers` endpoint scans all-time tracked messages (up to 5000) and returns aggregated domain counts.

### Configuration

No configuration. No AI required. Tracker detection at sync time uses the same domain matching logic.

---

## #78 Relationship Strength Scoring

Computes 5-factor relationship strength scores for all contacts in an account, classifies them into strength tiers, and persists results for fast retrieval.

### API

**POST** `/api/contacts/relationships/compute`

Triggers full score computation across all active accounts. Scores are written to `relationship_scores` (migration 032).

**Response:**
```json
{
  "computed": 38,
  "strong": 4,
  "regular": 12,
  "weak": 16,
  "dormant": 6
}
```

---

**GET** `/api/contacts/relationships/stats`

Returns aggregate statistics across all scored contacts.

**Response:**
```json
{
  "total_contacts": 38,
  "by_strength": {
    "strong": 4,
    "regular": 12,
    "weak": 16,
    "dormant": 6
  },
  "avg_score": 0.41,
  "most_active": {
    "email": "alice@example.com",
    "score": 0.91
  }
}
```

---

**GET** `/api/contacts/relationships/{email}`

Returns the detailed score for a single contact.

**Response:**
```json
{
  "email": "alice@example.com",
  "display_name": "Alice Smith",
  "strength_label": "strong",
  "overall_score": 0.873,
  "frequency_score": 0.920,
  "recency_score": 0.850,
  "reciprocity_score": 0.880,
  "response_time_score": 0.760,
  "thread_engagement_score": 0.600,
  "total_sent": 22,
  "total_received": 25,
  "avg_response_time_secs": 10800,
  "last_sent": 1710288000,
  "last_received": 1710200000,
  "first_interaction": 1680000000,
  "computed_at": 1710300000
}
```

Returns `404` if no score has been computed for this contact.

---

**GET** `/api/contacts/relationships?strength={label}&sort={field}&limit={n}&offset={n}`

Lists all scored contacts with filtering and pagination.

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `strength` | string | all | Filter by label: `strong`, `regular`, `weak`, `dormant` |
| `sort` | string | `score_desc` | Sort order: `score_desc`, `score_asc`, `name_asc`, `recent` |
| `limit` | int | 50 | Max results (capped at 200) |
| `offset` | int | 0 | Pagination offset |

**Response:**
```json
{
  "contacts": [
    {
      "email": "alice@example.com",
      "display_name": "Alice Smith",
      "strength_label": "strong",
      "overall_score": 0.873,
      "total_sent": 22,
      "total_received": 25,
      "avg_response_time_secs": 10800,
      "last_sent": 1710288000,
      "last_received": 1710200000,
      "first_interaction": 1680000000
    }
  ],
  "total": 38
}
```

### Strength Labels

| Label | Score Range | Meaning |
|-------|-------------|---------|
| `strong` | ≥ 0.70 | Frequent, recent, bidirectional communication |
| `regular` | 0.40 – 0.69 | Moderate ongoing contact |
| `weak` | 0.15 – 0.39 | Infrequent or one-sided |
| `dormant` | < 0.15 | Minimal or very old interaction |

### Scoring Factors

The overall score is a weighted sum of 5 factors, each normalized to 0–1:

| Factor | Weight | Algorithm |
|--------|--------|-----------|
| **Frequency** | 25% | Emails per week; ≥ 5/week = 1.0, linear below |
| **Recency** | 25% | Linear decay from last contact; today = 1.0, ≥ 90 days = 0.0 |
| **Reciprocity** | 20% | `min(sent, received) / max(sent, received)`; equal exchange = 1.0; all one-way = 0.0 |
| **Response Time** | 15% | Logarithmic scale; ≤ 1 hour = 1.0, ≥ 48 hours = 0.0 |
| **Thread Engagement** | 15% | Average thread depth; single message = 0.0, ≥ 5 messages/thread = 1.0 |

### How It Works

The `compute_detailed_scores` function runs per account and operates in 4 SQL passes:

1. **Received messages**: counts and timestamps for all non-draft, non-deleted messages where `from_address` is not the account owner.
2. **Sent messages**: scans the Sent folder, parses `to_addresses` JSON arrays to attribute sent counts to each recipient.
3. **Thread depth**: builds a `thread_id → message_count` map, then links each contact to their thread IDs to compute average depth.
4. **Response times**: loads all threaded messages ordered by `(thread_id, date)`, slides a 2-message window over each thread, records the time delta whenever sender alternates between contact and account owner.

Scores are persisted to `relationship_scores` via `INSERT OR REPLACE`, so re-computing always reflects the latest email data.

All email comparisons use `LOWER()` for case-insensitive matching. Scores are rounded to 3 decimal places.

### Configuration

No configuration. No AI required. Computation is on-demand via the POST endpoint; no automatic scheduling.
