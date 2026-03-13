# Wave 2 Batch 5: Feature Documentation

## #48 Relationship-Weighted Priority

Computes relationship strength scores based on email interaction patterns and uses them to boost priority for messages from close contacts.

### API

**POST** `/api/contacts/relationship-scores/compute`

Triggers computation of relationship scores for all contacts. Analyzes email frequency, recency, reply rate, bidirectionality, and thread depth.

**Response:**
```json
{
  "scored": 42
}
```

---

**GET** `/api/contacts/relationship-score/{email}`

Returns the relationship score for a specific contact.

| Param | Type | Description |
|-------|------|-------------|
| `email` | path | Contact email address (case-insensitive) |

**Response:**
```json
{
  "email": "alice@example.com",
  "score": 0.82,
  "frequency_score": 0.9,
  "recency_score": 0.75,
  "reply_rate_score": 0.85,
  "bidirectional_score": 1.0,
  "thread_depth_score": 0.6,
  "computed_at": 1710288000
}
```

Returns `404` if the contact has no computed score.

---

**GET** `/api/messages/prioritized`

Returns messages ranked by a blended score combining AI priority and relationship strength.

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `account_id` | string | all | Filter by account |
| `folder` | string | all | Filter by folder |
| `limit` | int | 50 | Max results (cap 200) |
| `offset` | int | 0 | Pagination offset |

**Response:**
```json
{
  "messages": [
    {
      "...MessageSummary fields...",
      "relationship_score": 0.82,
      "blended_score": 0.91
    }
  ],
  "total": 150
}
```

### How It Works

Relationship scores are computed from 5 signals: email frequency (volume of exchanges), recency (how recently you communicated), reply rate (% of their emails you replied to), bidirectionality (both sides sending), and thread depth (length of conversations). Each signal produces a 0-1 score; the final score is a weighted average. Scores are stored in a `relationship_scores` table.

The prioritized endpoint joins messages with relationship scores via `from_address` and computes `blended_score = (ai_priority_score * 0.6) + (relationship_score * 0.4)`, sorted descending.

### Configuration

Scores must be explicitly computed via the POST endpoint. No automatic scheduling.

---

## #54 CC/BCC Suggestions Based on Thread Context

Analyzes email threads to suggest relevant CC/BCC recipients based on historical co-occurrence patterns and AI reasoning.

### API

**POST** `/api/ai/suggest-cc`

**Request:**
```json
{
  "thread_id": "optional-thread-id",
  "to": ["alice@example.com"],
  "cc": ["bob@example.com"],
  "subject": "Q3 Planning",
  "body_preview": "Let's discuss the roadmap..."
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `thread_id` | string | no | Thread context for prior participants |
| `to` | array | yes | Current To recipients |
| `cc` | array | yes | Current CC recipients (excluded from suggestions) |
| `subject` | string | yes | Email subject |
| `body_preview` | string | yes | Email body preview |

**Response:**
```json
{
  "suggestions": [
    {
      "email": "carol@example.com",
      "name": "Carol Smith",
      "reason": "Frequently included in planning threads with Alice",
      "confidence": 0.85,
      "type": "cc"
    }
  ]
}
```

### How It Works

1. **Co-occurrence query:** Finds contacts who frequently appear in threads alongside the current recipients, using both `from_address` and parsed `cc_addresses`/`to_addresses` fields.
2. **Thread participants:** If `thread_id` is provided, includes all prior participants in that thread.
3. **AI reasoning:** Sends the co-occurrence candidates, subject, and body preview to the AI provider for intelligent filtering and reasoning about why each person should be CC'd.
4. **Deduplication:** Already-listed To/CC recipients are excluded. Max 5 suggestions returned, sorted by confidence.

### Configuration

Requires AI enabled. Co-occurrence data is derived from existing email history — no separate computation step needed.

---

## #58 Markdown Compose (Rich Email from Markdown)

Converts Markdown text to safe HTML for email composition, supporting tables, strikethrough, and nested formatting while stripping all XSS vectors.

### API

**POST** `/api/compose/markdown-preview`

**Request:**
```json
{
  "markdown": "# Hello\n\nThis is **bold** and ~~struck~~.\n\n| Col A | Col B |\n|-------|-------|\n| 1 | 2 |"
}
```

**Response:**
```json
{
  "html": "<h1>Hello</h1>\n<p>This is <strong>bold</strong> and <del>struck</del>.</p>\n<table>...</table>"
}
```

### How It Works

Uses `pulldown-cmark` with tables, strikethrough, and heading attributes enabled. The raw HTML output is then sanitized:

1. `<script>` tags are removed (including multiline)
2. `<iframe>` tags are removed
3. `on*` event handlers are stripped from all elements
4. `javascript:` URLs are neutralized (replaced with empty `href`)

Empty markdown input returns an empty HTML string.

### Configuration

No configuration. No AI required. Pure transformation endpoint.

---

## #63 Compose Email via Chat

Enables drafting emails through natural language conversation in the AI chat. Users describe what they want to write, and the AI composes a full draft with To/CC/Subject/Body.

### API

This feature works through the existing chat endpoint:

**POST** `/api/ai/chat`

When the user's message indicates intent to compose an email, the AI uses the `compose_email` tool internally. The result appears as a `proposed_action` requiring user confirmation before saving the draft.

**Tool Parameters (internal):**
```json
{
  "to": ["recipient@example.com"],
  "cc": ["cc@example.com"],
  "subject": "Meeting Follow-up",
  "body": "Hi team,\n\nFollowing up on...",
  "compose_mode": "new"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `to` | array | yes | Recipients |
| `cc` | array | no | CC recipients |
| `subject` | string | yes | Email subject |
| `body` | string | yes | Email body |
| `compose_mode` | string | yes | `"new"`, `"reply"`, or `"forward"` |

**POST** `/api/ai/chat/confirm/{action_id}` — Confirms the compose action, saving the draft to the database.

### How It Works

1. The `compose_email` tool is registered as one of 6 agentic tools available to the AI chat.
2. When the model decides to compose, it calls `compose_email` with the draft fields.
3. The handler validates required fields and creates a `ProposedAction` with `action_type: "compose_email"`.
4. The user reviews the draft in the chat UI and can confirm or reject.
5. On confirmation, `execute_compose_email()` saves the draft to the `drafts` table.

### Configuration

Requires AI enabled. The AI model must support tool use (Anthropic and OpenAI native; Ollama text fallback does not reliably trigger compose).

---

## #72 Social Engineering Detection

AI-powered analysis of incoming emails for social engineering manipulation tactics including urgency pressure, authority exploitation, fear/threats, reward lures, trust exploitation, and information harvesting.

### API

**POST** `/api/messages/detect-social-engineering`

**Request:**
```json
{
  "message_id": "msg-abc123"
}
```

Triggers AI analysis and caches the result for future GET requests.

**Response:**
```json
{
  "risk_level": "high",
  "tactics": [
    {
      "type": "urgency_pressure",
      "evidence": "Account will be suspended in 24 hours",
      "confidence": 0.92
    },
    {
      "type": "authority_exploitation",
      "evidence": "Claims to be from IT Security Department",
      "confidence": 0.85
    }
  ],
  "summary": "This email uses urgency and false authority to pressure the recipient into clicking a link."
}
```

---

**GET** `/api/messages/{id}/social-engineering`

Returns cached analysis result. Returns `404` if the message hasn't been analyzed yet.

### Risk Levels

| Level | Meaning |
|-------|---------|
| `none` | No manipulation detected |
| `low` | Minor persuasion techniques |
| `medium` | Moderate manipulation patterns |
| `high` | Strong social engineering indicators |
| `critical` | Multiple high-confidence tactics detected |

### Tactic Types

`urgency_pressure`, `authority_exploitation`, `fear_threat`, `reward_lure`, `trust_exploitation`, `information_harvesting`

### How It Works

1. Fetches the message body from the database.
2. Truncates to 5000 characters (char-safe) and sends to the AI provider with a cybersecurity-focused system prompt.
3. Parses the JSON response, filtering out invalid tactic types and clamping confidence to 0-1.
4. If all tactics are filtered out (invalid types), risk is downgraded to `"none"`.
5. Results are cached in `social_engineering_cache` table for subsequent GET requests.

### Configuration

Requires AI enabled. Analysis is on-demand (not automatic during sync). Results are cached permanently per message.

---

## #75 Data Loss Prevention (DLP) Scan

Scans outgoing email content for sensitive data (credit cards, SSNs, API keys, passwords, private keys, bank accounts) before sending, blocking or warning based on risk level.

### API

**POST** `/api/compose/dlp-scan`

**Request:**
```json
{
  "subject": "Here are the credentials",
  "body": "The API key is sk-abc123def456..."
}
```

**Response:**
```json
{
  "findings": [
    {
      "type": "api_key",
      "match": "sk-ab****56",
      "location": "body",
      "line": 1
    }
  ],
  "risk_level": "low",
  "allow_send": true
}
```

---

**POST** `/api/compose/dlp-scan` with override token:

**Request:**
```json
{
  "subject": "...",
  "body": "...",
  "override_token": "uuid-from-prior-scan"
}
```

If the token matches a previously generated token, `allow_send` is forced to `true` regardless of risk level. Tokens are single-use.

### Risk Levels

| Level | Trigger | `allow_send` |
|-------|---------|-------------|
| `none` | No findings | `true` |
| `low` | Single password, API key, or bank account | `true` |
| `high` | Credit card, SSN, private key, or 2+ findings of any type | `false` |

### Finding Types

| Type | Detection Method |
|------|-----------------|
| `credit_card` | 13-19 digit numbers passing Luhn validation |
| `ssn` | `XXX-XX-XXXX` format (excludes sequential/repeated) |
| `api_key` | Patterns: `sk-`, `AKIA`, `ghp_`, `xoxb-`, and others |
| `password` | `password:`, `passwd=`, `pwd=` followed by value |
| `private_key` | `-----BEGIN ... PRIVATE KEY-----` |
| `bank_account` | IBAN format (2 letter + 2 digit + up to 30 alphanumeric) |

### How It Works

1. Scans both subject and body using compiled regex patterns.
2. Credit cards are validated with Luhn algorithm (invalid checksums are not flagged).
3. SSNs filter out sequential (`123-45-6789`) and repeated (`111-11-1111`) patterns.
4. Matched values are masked (first 4 + stars + last 2 characters).
5. Risk level is determined by finding types and count.
6. An override token (UUID) is generated with each scan; submitting it back bypasses the block for single-use.

### Configuration

No configuration. No AI required. Pure regex-based detection with algorithmic validation.
