# Wave 2 Batch 8: Feature Documentation

## #82 Webhook Triggers

Register webhook URLs that fire on email events (received, sent, archived, deleted, labeled, starred). Supports CRUD management, test delivery, HMAC-SHA256 request signing, and automatic disabling after 10 consecutive delivery failures.

### API

**POST** `/api/webhooks`

Creates a new webhook registration.

**Request:**
```json
{
  "url": "https://example.com/hook",
  "events": ["received", "sent", "archived"],
  "secret": "my-signing-secret",
  "description": "Notify CRM on incoming mail"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `url` | string | yes | HTTPS endpoint to receive payloads |
| `events` | string[] | yes | Event types: `received`, `sent`, `archived`, `deleted`, `labeled`, `starred` |
| `secret` | string | no | Shared secret for HMAC-SHA256 signature |
| `description` | string | no | Human-readable label |

**Response:**
```json
{
  "id": "wh-abc123",
  "url": "https://example.com/hook",
  "events": ["received", "sent", "archived"],
  "description": "Notify CRM on incoming mail",
  "is_active": true,
  "consecutive_failures": 0,
  "created_at": 1710288000,
  "updated_at": 1710288000
}
```

Returns `400` if URL is not HTTPS or events array is empty/contains invalid types.

---

**GET** `/api/webhooks`

Lists all registered webhooks, ordered by creation date descending.

**Response:**
```json
[
  {
    "id": "wh-abc123",
    "url": "https://example.com/hook",
    "events": ["received", "sent", "archived"],
    "description": "Notify CRM on incoming mail",
    "is_active": true,
    "consecutive_failures": 0,
    "created_at": 1710288000,
    "updated_at": 1710288000
  }
]
```

---

**GET** `/api/webhooks/{id}`

Returns a single webhook by ID. Returns `404` if not found.

---

**PUT** `/api/webhooks/{id}`

Updates a webhook's URL, events, secret, description, or active state.

**Request:**
```json
{
  "url": "https://example.com/hook-v2",
  "events": ["received"],
  "is_active": true
}
```

All fields are optional. Reactivating a webhook resets `consecutive_failures` to 0. Returns `404` if not found.

---

**DELETE** `/api/webhooks/{id}`

Permanently deletes a webhook and all its delivery records. Returns `404` if not found.

---

**GET** `/api/webhooks/{id}/deliveries`

Returns delivery history for a webhook, newest first.

**Response:**
```json
{
  "deliveries": [
    {
      "id": "del-abc123",
      "webhook_id": "wh-abc123",
      "event_type": "received",
      "payload": "{\"message_id\":\"msg-123\",\"event\":\"received\"}",
      "response_status": 200,
      "response_body": "OK",
      "success": true,
      "delivered_at": 1710300000
    }
  ]
}
```

---

**POST** `/api/webhooks/{id}/test`

Sends a test payload to the webhook URL. The payload contains `{"event": "test", "webhook_id": "..."}`. Records the delivery attempt. Returns `404` if webhook not found.

**Response:**
```json
{
  "success": true,
  "response_status": 200,
  "response_body": "OK"
}
```

### Signing

Every delivery includes an `X-Iris-Signature` header computed as `HMAC-SHA256(secret, payload_json)`, hex-encoded. If no secret is configured, the header is omitted. Recipients verify by computing the same HMAC over the raw request body.

### Auto-Disable

Each successful delivery resets `consecutive_failures` to 0. Each failed delivery (non-2xx response or network error) increments the counter. When `consecutive_failures` reaches 10, the webhook is set to `is_active = false`. Reactivation via PUT resets the counter.

### Database

**Table:** `webhooks` (migration 040)

| Column | Type | Description |
|--------|------|-------------|
| `id` | TEXT PK | UUID |
| `url` | TEXT | Delivery endpoint |
| `events` | TEXT | JSON array of event types |
| `secret` | TEXT | HMAC signing secret (nullable) |
| `description` | TEXT | Human label (nullable) |
| `is_active` | INTEGER | 1 = active, 0 = disabled |
| `consecutive_failures` | INTEGER | Failure counter (default 0) |
| `created_at` | INTEGER | Creation epoch |
| `updated_at` | INTEGER | Last modification epoch |

**Table:** `webhook_deliveries` (migration 040)

| Column | Type | Description |
|--------|------|-------------|
| `id` | TEXT PK | UUID |
| `webhook_id` | TEXT FK | Parent webhook |
| `event_type` | TEXT | Event that triggered delivery |
| `payload` | TEXT | JSON payload sent |
| `response_status` | INTEGER | HTTP status code (nullable) |
| `response_body` | TEXT | Response body (nullable, truncated to 4KB) |
| `success` | INTEGER | 1 = success, 0 = failure |
| `delivered_at` | INTEGER | Delivery attempt epoch |

Index on `webhook_id` + `delivered_at DESC` for efficient history queries.

### Integration

- Webhook dispatch is triggered from the sync engine and message action handlers (archive, delete, label, star).
- Delivery is fire-and-forget with a 10-second HTTP timeout per attempt.
- No retry on individual failures -- the consecutive failure counter handles persistent outages.

### Configuration

No external dependencies. No AI required.

---

## #83 Structured Data Extraction

Extracts structured data from email content using a two-pass approach: regex first for deterministic patterns (amounts, dates, emails, URLs, tracking numbers), then AI for semantic extraction (invoice numbers, order IDs, event details). Results are queryable by type, date, and source message.

### API

**POST** `/api/extract/{message_id}`

Runs extraction on a specific message. Performs regex pass first, then AI semantic pass if enabled.

**Response:**
```json
{
  "extracted": [
    {
      "id": "ext-abc123",
      "message_id": "msg-123",
      "data_type": "amount",
      "value": "$1,234.56",
      "context": "Total due: $1,234.56 by March 15",
      "confidence": 1.0,
      "source": "regex",
      "created_at": 1710300000
    },
    {
      "id": "ext-def456",
      "message_id": "msg-123",
      "data_type": "tracking_number",
      "value": "1Z999AA10123456784",
      "context": "Your UPS tracking number is 1Z999AA10123456784",
      "confidence": 0.92,
      "source": "ai",
      "created_at": 1710300000
    }
  ],
  "regex_count": 4,
  "ai_count": 2
}
```

Returns `404` if message not found. Returns partial results (regex only) if AI is disabled.

---

**GET** `/api/extracted-data`

Lists extracted data with optional filters.

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `type` | string | all | Filter by data type: `amount`, `date`, `email`, `url`, `tracking_number`, `invoice`, `order_id`, `event` |
| `since` | int | none | Only entries created after this epoch |
| `message_id` | string | none | Filter to a specific message |

**Response:**
```json
{
  "data": [
    {
      "id": "ext-abc123",
      "message_id": "msg-123",
      "data_type": "amount",
      "value": "$1,234.56",
      "context": "Total due: $1,234.56 by March 15",
      "confidence": 1.0,
      "source": "regex",
      "created_at": 1710300000
    }
  ]
}
```

---

**GET** `/api/extracted-data/summary`

Returns aggregate counts grouped by data type.

**Response:**
```json
{
  "summary": {
    "amount": 12,
    "date": 8,
    "url": 23,
    "tracking_number": 3,
    "email": 15,
    "invoice": 2,
    "order_id": 1
  },
  "total": 64
}
```

---

**DELETE** `/api/extracted-data/{id}`

Deletes a single extracted data entry. Returns `404` if not found.

### Two-Pass Extraction

**Pass 1 -- Regex (deterministic):**
- **Amounts**: Currency patterns (`$1,234.56`, `USD 500`, `EUR 1.000,00`)
- **Dates**: Common date formats (ISO 8601, `MM/DD/YYYY`, `March 15, 2026`, relative like `next Friday`)
- **Emails**: RFC 5322 simplified pattern
- **URLs**: `https?://` with standard URL characters
- **Tracking numbers**: UPS (1Z), FedEx (12/15/20/22 digits), USPS (20-22 digits)

All regex matches get `confidence: 1.0` and `source: "regex"`.

**Pass 2 -- AI (semantic):**
Sends the email body to the AI provider with a system prompt requesting JSON extraction of invoice numbers, order IDs, event details, and any structured data the regex pass might miss. AI results get `confidence` from the model's assessment and `source: "ai"`. Duplicates (same type + value + message) are deduplicated, preferring the regex result.

### Database

**Table:** `extracted_data` (migration 041)

| Column | Type | Description |
|--------|------|-------------|
| `id` | TEXT PK | UUID |
| `message_id` | TEXT | Source message |
| `data_type` | TEXT | Type category |
| `value` | TEXT | Extracted value |
| `context` | TEXT | Surrounding text snippet (nullable) |
| `confidence` | REAL | 0.0-1.0 (1.0 for regex matches) |
| `source` | TEXT | `regex` or `ai` |
| `created_at` | INTEGER | Extraction epoch |

Unique constraint on `(message_id, data_type, value)`. Index on `data_type` for filtered queries.

### Integration

- Can be triggered manually per message or integrated into the sync pipeline via the job queue.
- AI pass uses the same `ProviderPool::generate` as other AI features.
- Context field stores up to 200 characters surrounding the match for display purposes.

### Configuration

Regex pass works without AI. AI pass requires `ai_enabled = true` and at least one configured provider.

---

## #85 Communication Health Reports

Generates periodic reports on email communication health: volume trends, response times, top contacts, category distribution, hourly send/receive patterns, read rate, and folder distribution. Includes AI-generated insights summarizing notable patterns.

### API

**POST** `/api/health-reports/generate`

Generates a new health report for a date range.

**Request:**
```json
{
  "date_from": 1709000000,
  "date_to": 1710000000,
  "account_id": "acc-123"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `date_from` | int | no | Start epoch (defaults to 30 days ago) |
| `date_to` | int | no | End epoch (defaults to now) |
| `account_id` | string | no | Filter to specific account |

**Response:**
```json
{
  "id": "hr-abc123",
  "period_start": 1709000000,
  "period_end": 1710000000,
  "metrics": {
    "total_received": 342,
    "total_sent": 87,
    "avg_response_time_hours": 4.2,
    "median_response_time_hours": 2.1,
    "read_rate": 0.89,
    "top_contacts": [
      {"email": "alice@example.com", "sent": 12, "received": 18}
    ],
    "category_distribution": {
      "Work": 145,
      "Newsletters": 89,
      "Finance": 34,
      "Social": 28,
      "Other": 46
    },
    "hourly_pattern": {
      "send": [0, 0, 0, 0, 0, 1, 3, 8, 12, 15, 11, 9, 7, 10, 12, 8, 5, 3, 2, 1, 0, 0, 0, 0],
      "receive": [2, 1, 1, 0, 1, 3, 8, 15, 22, 25, 20, 18, 14, 19, 21, 16, 12, 8, 5, 3, 2, 2, 1, 1]
    },
    "folder_distribution": {
      "INBOX": 210,
      "Archive": 98,
      "Sent": 87,
      "Trash": 34
    }
  },
  "insights": [
    "Your response time improved 15% compared to the previous period.",
    "You receive the most email between 9-10 AM -- consider blocking that hour for triage.",
    "89% read rate is above average. 11% of unread emails are in Newsletters category."
  ],
  "generated_at": 1710300000
}
```

Returns `503` if AI is disabled (insights generation requires AI; metrics are still computed).

---

**GET** `/api/health-reports`

Lists all generated reports, newest first.

**Response:**
```json
{
  "reports": [
    {
      "id": "hr-abc123",
      "period_start": 1709000000,
      "period_end": 1710000000,
      "total_received": 342,
      "total_sent": 87,
      "generated_at": 1710300000
    }
  ]
}
```

---

**GET** `/api/health-reports/{id}`

Returns the full report with all metrics and insights. Returns `404` if not found.

---

**DELETE** `/api/health-reports/{id}`

Permanently deletes a report. Returns `404` if not found.

### Database

**Table:** `health_reports` (migration 042)

| Column | Type | Description |
|--------|------|-------------|
| `id` | TEXT PK | UUID |
| `account_id` | TEXT | Account filter used (nullable = all accounts) |
| `period_start` | INTEGER | Report period start epoch |
| `period_end` | INTEGER | Report period end epoch |
| `metrics` | TEXT | JSON object with all computed metrics |
| `insights` | TEXT | JSON array of AI-generated insight strings |
| `generated_at` | INTEGER | Generation epoch |

### How It Works

Report generation runs several SQL aggregation queries against the `messages` table:
1. **Volume**: COUNT with folder/direction filters.
2. **Response times**: Joins sent messages to their thread's most recent inbound message, computes time delta.
3. **Top contacts**: GROUP BY `from_address` / `to_addresses`, ranked by total interaction count (top 10).
4. **Category distribution**: GROUP BY `ai_category`.
5. **Hourly patterns**: Extracts hour from `date` epoch, counts per hour bucket (0-23) split by sent/received.
6. **Read rate**: `COUNT(is_read = 1) / COUNT(*)` for received messages.
7. **Folder distribution**: GROUP BY `folder`.

After metrics computation, the full metrics JSON is sent to the AI provider with a system prompt requesting 3-5 actionable insights. Insights are stored as a JSON array. If AI is unavailable, insights default to an empty array.

### Integration

- Uses AI classification data (`ai_category`) for category distribution.
- Response time calculation reuses the same thread-based approach as feature #80 (Response Time Patterns).
- Reports are immutable once generated -- regenerate for updated data.

### Configuration

Metrics computation works without AI. Insights require `ai_enabled = true` and at least one configured provider.

---

## #39 Newsletter Feed View

Magazine-style browsing interface for newsletter emails. Auto-discovers newsletters from `List-Unsubscribe` headers or AI category classification. Groups newsletters by sender into feeds, tracks unread counts, supports favorites and muting.

### API

**GET** `/api/newsletter-feeds`

Lists all discovered newsletter feeds, ordered by last received date descending.

**Response:**
```json
{
  "feeds": [
    {
      "id": "nf-abc123",
      "sender_address": "newsletter@techcrunch.com",
      "sender_name": "TechCrunch Daily",
      "unread_count": 3,
      "total_count": 47,
      "is_favorite": true,
      "is_muted": false,
      "last_received_at": 1710288000,
      "created_at": 1709000000
    }
  ]
}
```

---

**POST** `/api/newsletter-feeds/discover`

Scans the inbox for newsletter senders not yet tracked as feeds. Detects newsletters by `List-Unsubscribe` header presence or `ai_category = 'Newsletters'`. Creates a feed entry for each new sender.

**Response:**
```json
{
  "discovered": 5,
  "total_feeds": 12
}
```

---

**PUT** `/api/newsletter-feeds/{id}`

Updates feed preferences.

**Request:**
```json
{
  "is_favorite": true,
  "is_muted": false
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `is_favorite` | bool | no | Pin to favorites section |
| `is_muted` | bool | no | Hide from default feed list (still tracked) |

Returns `404` if feed not found.

---

**DELETE** `/api/newsletter-feeds/{id}`

Removes a feed from tracking. Does not delete the underlying emails. Returns `404` if not found.

---

**GET** `/api/newsletter-feeds/{id}/articles`

Lists emails belonging to a feed, newest first.

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `limit` | int | 20 | Max results (capped at 100) |
| `offset` | int | 0 | Pagination offset |

**Response:**
```json
{
  "articles": [
    {
      "message_id": "msg-abc123",
      "subject": "This Week in AI",
      "snippet": "OpenAI announced GPT-5, Google responds with...",
      "date": 1710288000,
      "is_read": false
    }
  ],
  "total": 47
}
```

---

**POST** `/api/newsletter-feeds/{id}/mark-read`

Marks all unread articles in the feed as read. Updates both the feed's `unread_count` and the underlying messages' `is_read` flag.

**Response:**
```json
{
  "marked": 3
}
```

Returns `404` if feed not found.

### Database

**Table:** `newsletter_feeds` (migration 043)

| Column | Type | Description |
|--------|------|-------------|
| `id` | TEXT PK | UUID |
| `sender_address` | TEXT UNIQUE | Newsletter sender email |
| `sender_name` | TEXT | Display name (nullable) |
| `unread_count` | INTEGER | Cached unread count (default 0) |
| `total_count` | INTEGER | Cached total count (default 0) |
| `is_favorite` | INTEGER | 1 = favorited |
| `is_muted` | INTEGER | 1 = muted |
| `last_received_at` | INTEGER | Most recent email epoch (nullable) |
| `created_at` | INTEGER | Discovery epoch |

Unique constraint on `sender_address`.

### How It Works

Discovery queries `SELECT DISTINCT from_address, from_name FROM messages WHERE raw_headers LIKE '%List-Unsubscribe%' OR ai_category = 'Newsletters'`, then inserts new feeds via `ON CONFLICT(sender_address) DO NOTHING`. Counts are computed at discovery time and refreshed when articles are listed. The articles endpoint joins `newsletter_feeds.sender_address` to `messages.from_address` with ordering by date.

Mark-read updates `messages SET is_read = 1 WHERE from_address = ? AND is_read = 0`, then recomputes `unread_count`.

### Integration

- Uses AI classification data (`ai_category`) from the V6 AI pipeline for newsletter detection.
- Shares newsletter detection logic with feature #66 (Newsletter Digest Generation).
- Muted feeds are excluded from the default GET listing but can be retrieved with a `?include_muted=true` query param.

### Configuration

No AI required. Newsletter detection uses header inspection and existing AI classification data.

---

## #40 Subscription Management Dashboard

Centralized dashboard for tracking and managing email subscriptions. Scans the inbox to discover recurring senders, tracks email frequency and read rates, and provides bulk actions for archiving, blocking, or unsubscribing.

### API

**GET** `/api/subscriptions`

Lists all tracked subscriptions, ordered by email count descending.

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `status` | string | all | `active`, `archived`, `blocked`, `unsubscribed` |
| `sort` | string | `count` | `count`, `frequency`, `read_rate`, `last_received` |
| `limit` | int | 50 | Max results (capped at 200) |
| `offset` | int | 0 | Pagination offset |

**Response:**
```json
{
  "subscriptions": [
    {
      "id": "sub-abc123",
      "sender_address": "deals@shop.com",
      "sender_name": "ShopDaily",
      "email_count": 89,
      "read_count": 12,
      "read_rate": 0.13,
      "avg_frequency_days": 1.2,
      "last_received_at": 1710288000,
      "has_unsubscribe": true,
      "unsubscribe_link": "https://shop.com/unsubscribe?token=abc",
      "status": "active",
      "created_at": 1709000000,
      "updated_at": 1710288000
    }
  ],
  "total": 34
}
```

---

**POST** `/api/subscriptions/scan`

Scans the inbox to discover or update subscription senders. Identifies senders with 5+ emails or a `List-Unsubscribe` header. Computes frequency and read rate for each.

**Response:**
```json
{
  "scanned_senders": 142,
  "new_subscriptions": 8,
  "updated_subscriptions": 26
}
```

---

**GET** `/api/subscriptions/{id}`

Returns full details for a single subscription. Returns `404` if not found.

---

**PUT** `/api/subscriptions/{id}/status`

Updates subscription status.

**Request:**
```json
{
  "status": "archived"
}
```

Valid statuses: `active`, `archived`, `blocked`, `unsubscribed`. Returns `400` for invalid status. Returns `404` if not found.

---

**POST** `/api/subscriptions/bulk-action`

Applies an action to multiple subscriptions at once.

**Request:**
```json
{
  "ids": ["sub-abc123", "sub-def456"],
  "action": "archive"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `ids` | string[] | yes | Subscription IDs (max 50) |
| `action` | string | yes | `archive`, `block`, or `unsubscribe` |

**Response:**
```json
{
  "affected": 2
}
```

`archive` sets status to `archived`. `block` sets status to `blocked` and creates a filter to auto-archive future emails. `unsubscribe` sets status to `unsubscribed` (actual unsubscribe via link is left to the user).

Returns `400` if IDs exceed 50 or action is invalid.

---

**GET** `/api/subscriptions/stats`

Returns aggregate subscription statistics.

**Response:**
```json
{
  "total_subscriptions": 34,
  "active": 22,
  "archived": 8,
  "blocked": 3,
  "unsubscribed": 1,
  "avg_read_rate": 0.42,
  "lowest_read_rate": {
    "sender": "deals@shop.com",
    "read_rate": 0.05
  },
  "highest_frequency": {
    "sender": "alerts@monitoring.io",
    "avg_frequency_days": 0.3
  },
  "total_emails_from_subscriptions": 1847
}
```

### Database

**Table:** `subscriptions` (migration 044)

| Column | Type | Description |
|--------|------|-------------|
| `id` | TEXT PK | UUID |
| `sender_address` | TEXT UNIQUE | Subscription sender email |
| `sender_name` | TEXT | Display name (nullable) |
| `email_count` | INTEGER | Total emails from sender |
| `read_count` | INTEGER | Read emails from sender |
| `read_rate` | REAL | `read_count / email_count` |
| `avg_frequency_days` | REAL | Average days between emails |
| `last_received_at` | INTEGER | Most recent email epoch |
| `has_unsubscribe` | INTEGER | 1 = List-Unsubscribe header found |
| `unsubscribe_link` | TEXT | Extracted unsubscribe URL (nullable) |
| `status` | TEXT | `active`, `archived`, `blocked`, `unsubscribed` |
| `created_at` | INTEGER | Discovery epoch |
| `updated_at` | INTEGER | Last scan update epoch |

Unique constraint on `sender_address`. Index on `status`.

**Module:** `src/api/subscription_management.rs`

### How It Works

The scan endpoint queries `SELECT from_address, from_name, COUNT(*) as cnt, SUM(is_read) as read_cnt FROM messages GROUP BY LOWER(from_address) HAVING cnt >= 5 OR raw_headers LIKE '%List-Unsubscribe%'`. For each sender, it computes `avg_frequency_days` from the date range divided by email count. Unsubscribe links are extracted by parsing the `List-Unsubscribe` header for `<https://...>` patterns. Results are upserted into the `subscriptions` table.

Read rate is `read_count / email_count`, computed at scan time and cached. The stats endpoint aggregates across all subscriptions.

Bulk `block` action additionally creates an entry in the existing message filter system to auto-archive future emails from the sender.

### Integration

- Shares newsletter sender discovery logic with features #39 (Newsletter Feed View) and #67 (Subscription Audit).
- Unsubscribe links are extracted from `List-Unsubscribe` headers -- the same parsing used in feature #68 (Unsubscribe Detection).
- Block action integrates with the message filter pipeline for automatic archival.

### Configuration

No AI required. Pure statistical analysis from inbox data.

---

## #84 Email Analytics Dashboard

Real-time email analytics with overview metrics, volume-over-time charts, category distribution, top contacts, hourly patterns, and response time tracking. Supports daily snapshots for historical trend analysis.

### API

**GET** `/api/analytics/overview`

Returns high-level inbox metrics.

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `days` | int | 30 | Lookback period in days (max 365) |
| `account_id` | string | all | Filter to specific account |

**Response:**
```json
{
  "total_received": 892,
  "total_sent": 231,
  "avg_per_day_received": 29.7,
  "avg_per_day_sent": 7.7,
  "unread_count": 42,
  "read_rate": 0.95,
  "avg_response_time_hours": 3.8,
  "period_days": 30
}
```

---

**GET** `/api/analytics/volume`

Returns daily email volume over time.

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `days` | int | 30 | Lookback period |
| `account_id` | string | all | Filter to account |

**Response:**
```json
{
  "volume": [
    {"date": "2026-03-12", "received": 32, "sent": 8},
    {"date": "2026-03-11", "received": 28, "sent": 11}
  ]
}
```

---

**GET** `/api/analytics/categories`

Returns message distribution by AI category.

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `days` | int | 30 | Lookback period |

**Response:**
```json
{
  "categories": [
    {"category": "Work", "count": 312, "percentage": 0.35},
    {"category": "Newsletters", "count": 178, "percentage": 0.20},
    {"category": "Finance", "count": 89, "percentage": 0.10}
  ]
}
```

---

**GET** `/api/analytics/top-contacts`

Returns most active contacts by interaction volume.

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `days` | int | 30 | Lookback period |
| `limit` | int | 10 | Max contacts (capped at 50) |

**Response:**
```json
{
  "contacts": [
    {
      "email": "alice@example.com",
      "name": "Alice Smith",
      "sent_to": 15,
      "received_from": 22,
      "total": 37
    }
  ]
}
```

---

**GET** `/api/analytics/hourly-distribution`

Returns email volume by hour of day (0-23).

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `days` | int | 30 | Lookback period |

**Response:**
```json
{
  "hours": [
    {"hour": 0, "received": 5, "sent": 0},
    {"hour": 1, "received": 3, "sent": 0},
    {"hour": 9, "received": 45, "sent": 18}
  ]
}
```

Array always contains 24 entries (hours 0-23).

---

**GET** `/api/analytics/response-times`

Returns response time distribution and trends.

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `days` | int | 30 | Lookback period |

**Response:**
```json
{
  "avg_hours": 3.8,
  "median_hours": 2.1,
  "p95_hours": 12.5,
  "distribution": {
    "under_1h": 42,
    "1h_to_4h": 68,
    "4h_to_24h": 31,
    "over_24h": 12
  },
  "trend": [
    {"date": "2026-03-12", "avg_hours": 3.2},
    {"date": "2026-03-11", "avg_hours": 4.1}
  ]
}
```

---

**POST** `/api/analytics/snapshot`

Creates a daily analytics snapshot for the current date. Captures all overview metrics and stores them for historical comparison.

**Response:**
```json
{
  "id": "snap-abc123",
  "date": "2026-03-13",
  "total_received": 32,
  "total_sent": 8,
  "unread_count": 42,
  "read_rate": 0.95,
  "avg_response_time_hours": 3.8,
  "created_at": 1710300000
}
```

Returns `409` if a snapshot for today already exists.

### Database

**Table:** `analytics_snapshots` (migration 045)

| Column | Type | Description |
|--------|------|-------------|
| `id` | TEXT PK | UUID |
| `snapshot_date` | TEXT UNIQUE | ISO date (`YYYY-MM-DD`) |
| `account_id` | TEXT | Account (nullable = all accounts) |
| `total_received` | INTEGER | Emails received that day |
| `total_sent` | INTEGER | Emails sent that day |
| `unread_count` | INTEGER | Unread count at snapshot time |
| `read_rate` | REAL | Read rate at snapshot time |
| `avg_response_time_hours` | REAL | Avg response time (nullable) |
| `metrics_json` | TEXT | Full metrics blob for future extensibility |
| `created_at` | INTEGER | Snapshot epoch |

Unique constraint on `(snapshot_date, account_id)`.

### How It Works

All analytics endpoints run SQL aggregation queries against the `messages` table with date filtering (`WHERE date >= ?`). Dates are converted from epoch to date strings via `strftime('%Y-%m-%d', date, 'unixepoch')` for daily grouping.

**Volume**: Groups by date, counts sent (folder in Sent variants) vs received.
**Categories**: Groups by `ai_category`, computes percentage against total.
**Top contacts**: Unions sent-to and received-from, groups by email, sums interactions.
**Hourly**: Extracts hour via `strftime('%H', date, 'unixepoch')`, counts per bucket.
**Response times**: Joins sent messages to their thread's preceding inbound message, computes time deltas, buckets into distribution ranges.

Snapshots capture a point-in-time record. The `metrics_json` column stores the full overview output for forward compatibility.

### Integration

- Uses AI classification data (`ai_category`) for category analytics.
- Response time logic shares the thread-based approach with features #80 (Response Time Patterns) and #85 (Health Reports).
- Snapshots can be triggered by a cron-like scheduler or manually via the API.

### Configuration

No AI required. All endpoints are pure SQL aggregations over existing message data.
