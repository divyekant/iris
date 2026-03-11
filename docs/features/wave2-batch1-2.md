# Wave 2 Batch 1-2: Feature Documentation

## #44 Needs-Reply Detection + Queue

Automatically flags incoming emails that expect a response (questions, requests, action items) and provides a dedicated queue endpoint.

### API

**GET** `/api/messages/needs-reply`

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `account_id` | string | all | Filter by account |
| `limit` | int | 50 | Max results (cap 500) |
| `offset` | int | 0 | Pagination offset |

**Response:**
```json
{
  "messages": [MessageSummary],
  "total": 42
}
```

### How It Works

The AI classification pipeline (`src/ai/pipeline.rs`) outputs `needs_reply: bool` as part of the single-prompt classification. The model evaluates whether the sender is asking a question, making a request, or expecting a response. The flag is stored as `ai_needs_reply` on the `messages` table. The queue endpoint filters for `ai_needs_reply = 1 AND is_read = 0 AND is_deleted = 0`, ordered by date DESC.

### Configuration

Requires AI to be enabled (`ai_enabled = true` in config). Detection runs automatically during email sync via the job queue (`ai_classify` job type).

---

## #46 Sentiment Analysis on Incoming Email

Classifies the emotional tone of each incoming email as positive, negative, neutral, or mixed.

### API

Sentiment is returned as part of the standard message endpoints:

- **GET** `/api/messages` -- `ai_sentiment` field on each `MessageSummary`
- **GET** `/api/messages/{id}` -- `ai_sentiment` field on `MessageDetail`

**Sentiment values:** `"positive"`, `"negative"`, `"neutral"`, `"mixed"`, or `null` (unclassified).

### How It Works

Sentiment is extracted by the same single-prompt AI classification pipeline that handles intent, priority, and category (`src/ai/pipeline.rs`). The system prompt instructs the model to output `"sentiment": "positive|negative|neutral|mixed"` as part of the JSON response. Stored in the `ai_sentiment` column on `messages`.

### Configuration

No separate configuration. Runs whenever AI classification is enabled.

---

## #49 Thread Importance Decay Over Time

Automatically reduces priority scores on older messages so stale threads sink in importance, keeping the inbox focused on recent items.

### API

**GET** `/api/config/ai` -- returns current decay settings:
```json
{
  "decay_enabled": true,
  "decay_threshold_days": 7,
  "decay_factor": 0.85
}
```

**PUT** `/api/config/ai` -- update decay settings:
```json
{
  "decay_enabled": true,
  "decay_threshold_days": 7,
  "decay_factor": 0.85
}
```

| Setting | Type | Default | Constraints |
|---------|------|---------|-------------|
| `decay_enabled` | bool | `true` | -- |
| `decay_threshold_days` | int | 7 | >= 1 |
| `decay_factor` | float | 0.85 | 0.0 - 1.0 |

### How It Works

The background job worker runs priority decay every hour (`src/jobs/worker.rs`). It reads the three config values from the DB, then calls `message::decay_priority_scores()` which multiplies `ai_priority_score` by `decay_factor` for all messages older than `threshold_days` that haven't already decayed below a floor. Priority labels are downgraded when scores cross thresholds.

### Configuration

Configured via Settings UI or `PUT /api/config/ai`. Decay is enabled by default. Set `decay_enabled: false` to disable.

---

## #51 Draft from Intent (Plain English -> Full Email)

Generates a complete, polished email draft from a plain-English description of what the user wants to say.

### API

**POST** `/api/ai/draft-from-intent`

**Request:**
```json
{
  "intent": "tell Bob I can't make Tuesday's meeting, suggest Thursday instead",
  "context": "From: Bob\nSubject: Meeting Tuesday\nHey, can you make it?"
}
```

| Field | Type | Required | Constraints |
|-------|------|----------|-------------|
| `intent` | string | yes | Max 2000 chars, non-empty |
| `context` | string | no | Optional reply context (original email) |

**Response:**
```json
{
  "subject": "Re: Meeting Tuesday",
  "body": "Hi Bob,\n\nUnfortunately I won't be able to make Tuesday's meeting...",
  "suggested_to": ["bob@example.com"]
}
```

### How It Works

The intent is sent to the AI provider with a system prompt instructing it to output a JSON object with `subject`, `body`, and `suggested_to`. If `context` is provided (e.g., replying to an existing email), it's appended to the prompt so the model can write a contextually appropriate reply. The response is parsed with fallback handling for markdown fences and embedded JSON.

### Configuration

Requires AI enabled. Max intent length: 2000 characters. Max content: 50KB.

---

## #55 Subject Line Generation/Improvement

Suggests 3 subject lines for a given email body, or improves an existing subject.

### API

**POST** `/api/ai/suggest-subject`

**Request:**
```json
{
  "body": "Please find attached the Q3 financial report...",
  "current_subject": "stuff"
}
```

| Field | Type | Required | Constraints |
|-------|------|----------|-------------|
| `body` | string | yes | Non-empty, max 50KB |
| `current_subject` | string | no | Existing subject to improve |

**Response:**
```json
{
  "suggestions": [
    "Q3 Financial Report - Review Requested",
    "Attached: Q3 Financial Summary",
    "Q3 Report Ready for Review"
  ]
}
```

### How It Works

The email body is truncated to 3000 chars, combined with the optional current subject, and sent to the AI provider. The system prompt requests a JSON array of 3 concise subject lines. Response parsing handles JSON arrays, embedded JSON within prose, and numbered-list fallback (strips leading numbers/bullets).

### Configuration

Requires AI enabled. Body is truncated to 3000 characters for the prompt.

---

## #56 Grammar & Tone Check (Before-Send)

Analyzes a draft email for grammar, spelling, tone, and clarity issues before sending.

### API

**POST** `/api/ai/grammar-check`

**Request:**
```json
{
  "content": "Their going to the meeting tommorow...",
  "subject": "Meting Notes"
}
```

| Field | Type | Required | Constraints |
|-------|------|----------|-------------|
| `content` | string | yes | Non-empty, max 50KB |
| `subject` | string | no | Included in analysis if present |

**Response:**
```json
{
  "score": 72,
  "tone": "casual",
  "issues": [
    {
      "kind": "grammar",
      "description": "Subject-verb disagreement",
      "suggestion": "Use 'are' instead of 'is'"
    },
    {
      "kind": "spelling",
      "description": "Misspelled word",
      "suggestion": "Use 'tomorrow' instead of 'tommorow'"
    }
  ],
  "improved_content": "They're going to the meeting tomorrow..."
}
```

**Issue kinds:** `grammar`, `spelling`, `tone`, `clarity`, `punctuation`.

**Score:** 0-100 (capped at 100). A score of 100 with empty issues means the email is perfect.

### How It Works

The email content (with optional subject prepended) is sent to the AI provider with a system prompt requesting JSON analysis. The response includes a quality score, detected tone, list of issues with suggestions, and an optional improved version of the full email. Markdown code fences in AI responses are automatically stripped.

### Configuration

Requires AI enabled. No additional configuration.

---

## #59 On-Demand Briefing ("Brief Me on Today")

Generates an AI-powered daily email briefing summarizing today's inbox activity, highlighting urgent items and top senders.

### API

**GET** `/api/ai/briefing`

**Response:**
```json
{
  "summary": "You have 15 emails today, 8 unread. 2 urgent emails need attention, including...",
  "stats": {
    "total_today": 15,
    "unread": 8,
    "needs_reply": 3,
    "urgent": 2
  },
  "highlights": [
    {
      "message_id": "abc123",
      "from": "Alice",
      "subject": "Server is down",
      "reason": "urgent"
    }
  ]
}
```

**Highlight reasons:** `"urgent"`, `"needs_reply"`, `"high_priority"`.

### How It Works

1. **Stats query:** Counts today's emails, unread count, urgent count (ai_priority_label = 'urgent'), and needs-reply count (ai_intent = 'ACTION_REQUEST').
2. **Highlights query:** Selects up to 10 messages that are urgent, high-priority, or action-requested, ordered by urgency.
3. **Top senders:** Groups today's messages by sender, returns top 5 by count.
4. **AI narrative:** All data is compiled into a prompt and sent to the AI provider to generate a 3-5 sentence natural-language summary. If the AI provider is unavailable, a structured fallback summary is generated from the stats.

### Configuration

Requires AI enabled. "Today" is computed from midnight local time. No caching -- each request generates a fresh briefing.

---

## #67 Subscription Audit (Surface Never-Opened Subscriptions)

Identifies subscription/newsletter senders with low read rates, helping users find and clean up email clutter.

### API

**GET** `/api/subscriptions/audit`

**Response:**
```json
{
  "subscriptions": [
    {
      "sender": "newsletter@spam.com",
      "sender_name": "Spam Newsletter",
      "total_count": 48,
      "read_count": 3,
      "read_rate": 0.0625,
      "last_received": 1709500800,
      "has_unsubscribe": true,
      "category": "Newsletters"
    }
  ]
}
```

### How It Works

Groups all non-deleted, non-draft messages by `from_address`. Only senders with 3+ emails appear (the subscription threshold). Results are ordered by read rate ascending (least-read first). The `has_unsubscribe` flag is derived from checking `raw_headers` for `List-Unsubscribe` header presence. Returns up to 50 senders.

### Configuration

No configuration needed. The threshold (3+ emails from same sender) is hardcoded. Does not require AI to be enabled.

---

## #68 One-Click Unsubscribe

Enables unsubscribing from mailing lists directly from the email client, supporting RFC 8058 one-click, URL redirect, and mailto methods.

### API

**POST** `/api/messages/{id}/unsubscribe`

**Response:**
```json
{
  "success": true,
  "method": "one-click",
  "url": "https://lists.example.com/unsubscribe"
}
```

**Methods:**
| Method | Behavior |
|--------|----------|
| `one-click` | Server sends POST with `List-Unsubscribe=One-Click` body (RFC 8058). Returns success/failure. |
| `url` | Returns the HTTP URL for the frontend to open in a new tab. |
| `mailto` | Returns the mailto: URL for the frontend to handle. |

**Errors:**
- `404` -- Message not found or has no `List-Unsubscribe` header.
- `400` -- Unsubscribe URL format not recognized.
- `502` -- One-click POST request to the list server failed.

### How It Works

During IMAP sync, `List-Unsubscribe` and `List-Unsubscribe-Post` headers are parsed and stored in `list_unsubscribe` and `list_unsubscribe_post` columns on the `messages` table. When the unsubscribe endpoint is called:
1. If `list_unsubscribe_post` is true and URL is HTTP, sends a POST request with `List-Unsubscribe=One-Click` body per RFC 8058.
2. If URL is HTTP without POST support, returns the URL for browser-based unsubscribe.
3. If URL is `mailto:`, returns it for the frontend to handle.

### Configuration

No configuration needed. Requires the sending list to include `List-Unsubscribe` headers in their emails.

---

## #73 Impersonation Detection (Lookalike Domains)

Detects sender domains that visually resemble known trusted domains (e.g., `paypa1.com` mimicking `paypal.com`), warning users of potential phishing.

### API

Impersonation risk is returned as part of the message detail endpoint:

**GET** `/api/messages/{id}`

```json
{
  "...message fields...",
  "impersonation_risk": {
    "lookalike_of": "paypal.com",
    "risk_level": "high"
  }
}
```

`impersonation_risk` is `null` when the domain is safe (exact match with known domain or unrelated).

**Risk levels:**
| Level | Trigger |
|-------|---------|
| `high` | Levenshtein distance 1 from a known domain, or homoglyph match |
| `medium` | Levenshtein distance 2 from a known domain |

### How It Works

When a message detail is requested, the sender's domain is extracted and checked against a hardcoded list of 25+ known trusted domains (Gmail, PayPal, Amazon, GitHub, etc.). Two detection methods are applied:

1. **Homoglyph detection:** Normalizes the suspect domain using common visual substitutions (`rn`->`m`, `vv`->`w`, `cl`->`d`, `0`->`o`, `1`->`l`/`i`, `5`->`s`) and checks if the normalized form matches a known domain.
2. **Levenshtein distance:** Computes edit distance between the suspect and each known domain. Distance 1 = high risk, distance 2 = medium risk.

Exact matches (e.g., actual `gmail.com`) immediately return no risk. Implementation is in `src/api/trust.rs`.

### Configuration

No configuration. The known domains list is compiled into the binary. Does not require AI to be enabled.
