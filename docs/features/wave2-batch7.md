# Wave 2 Batch 7: Feature Documentation

## #64 Auto-Archive Patterns

Analyzes the user's archive history to detect recurring patterns by sender, category, or sender+category combination. When a sender's emails are archived >= 80% of the time with at least 5 matches, a pattern is created. These patterns can then be used to suggest or auto-archive matching new messages.

### API

**POST** `/api/ai/archive-patterns/compute`

Scans all messages to detect sender, category, and sender+category archive patterns. Creates new patterns or updates existing ones via upsert (`ON CONFLICT ... DO UPDATE`).

**Response:**
```json
{
  "patterns_created": 3,
  "patterns_updated": 1
}
```

---

**GET** `/api/ai/archive-patterns`

Lists all detected patterns, ordered by confidence descending then match count descending.

**Response:**
```json
[
  {
    "id": "a1b2c3d4-...",
    "pattern_type": "sender",
    "pattern_value": "newsletter@spam.com",
    "confidence": 0.857,
    "match_count": 6,
    "total_from_sender": 7,
    "archive_rate": 0.857,
    "is_active": true,
    "created_at": 1710288000,
    "updated_at": 1710288000
  },
  {
    "id": "e5f6g7h8-...",
    "pattern_type": "sender_category",
    "pattern_value": "alerts@service.com::Updates",
    "confidence": 1.0,
    "match_count": 6,
    "total_from_sender": 6,
    "archive_rate": 1.0,
    "is_active": true,
    "created_at": 1710288000,
    "updated_at": 1710288000
  }
]
```

---

**POST** `/api/ai/archive-patterns/suggest`

Given a list of message IDs, checks each against active patterns and returns suggestions for which messages should be archived.

**Request:**
```json
{
  "message_ids": ["msg-abc123", "msg-def456"]
}
```

**Response:**
```json
{
  "suggestions": [
    {
      "message_id": "msg-abc123",
      "pattern_id": "a1b2c3d4-...",
      "reason": "Emails from newsletter@spam.com are typically archived",
      "confidence": 0.857
    }
  ]
}
```

Only the highest-confidence pattern match is returned per message. Disabled patterns (`is_active = 0`) are excluded.

---

**PUT** `/api/ai/archive-patterns/{id}`

Updates a pattern's active state or confidence threshold.

**Request:**
```json
{
  "is_active": false,
  "confidence": 0.9
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `is_active` | bool | no | Enable or disable the pattern |
| `confidence` | float | no | Override confidence (must be 0.0-1.0) |

**Response:**
```json
{
  "updated": true
}
```

Returns `400` if confidence is out of range, `404` if pattern not found.

---

**DELETE** `/api/ai/archive-patterns/{id}`

Permanently deletes a pattern (user opt-out).

**Response:** `404` if not found.

### Database

**Table:** `archive_patterns` (migration 034)

| Column | Type | Description |
|--------|------|-------------|
| `id` | TEXT PK | UUID |
| `pattern_type` | TEXT | `sender`, `category`, `sender_category`, or `subject_pattern` |
| `pattern_value` | TEXT | Sender email, category name, or `sender::category` combo |
| `confidence` | REAL | Archive rate used as confidence (0.0-1.0) |
| `match_count` | INTEGER | Number of archived messages matching |
| `total_from_sender` | INTEGER | Total messages from source |
| `archive_rate` | REAL | `match_count / total_from_sender` |
| `is_active` | INTEGER | 1 = active, 0 = disabled |

Unique constraint on `(pattern_type, pattern_value)`.

### How It Works

The `compute_patterns_impl` function runs three SQL passes:

1. **Sender patterns**: Groups messages by `from_address`, counts archived vs total, creates a pattern when `archive_rate >= 0.8` and `archived_count >= 5`.
2. **Category patterns**: Same logic grouped by `ai_category`.
3. **Sender+Category combos**: Groups by `from_address || '::' || ai_category`, same thresholds.

The suggest endpoint loads all active patterns sorted by confidence, then for each message checks `from_address` and `ai_category` against patterns. Only the first (highest-confidence) match per message is returned.

### Integration

- Uses AI classification data (`ai_category` from the V6 AI pipeline) for category-based patterns.
- Patterns apply to any account's messages in the database.
- No AI provider needed -- pure statistical analysis from archive history.

### Configuration

Thresholds are hardcoded constants: `MIN_ARCHIVE_RATE = 0.8`, `MIN_MATCH_COUNT = 5`. No runtime configuration.

---

## #66 Newsletter Digest Generation

Consolidates newsletter and subscription emails into AI-generated digest summaries. Detects newsletters by the presence of a `List-Unsubscribe` header or `ai_category` being "Newsletters" or "Promotions". Groups emails by sender and feeds them to the AI provider to produce a titled, source-organized summary.

### API

**POST** `/api/ai/newsletter-digest`

Generates a new digest from newsletter emails matching the filter criteria.

**Request:**
```json
{
  "date_from": 1709000000,
  "date_to": 1710000000,
  "senders": ["newsletter@example.com"],
  "max_emails": 50
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `date_from` | int | no | Unix epoch lower bound |
| `date_to` | int | no | Unix epoch upper bound |
| `senders` | string[] | no | Filter to specific senders |
| `max_emails` | int | no | Max emails to include (default 100, cap 500) |

**Response:**
```json
{
  "id": "digest-abc123",
  "title": "Weekly Tech & Commerce Digest",
  "summary": "## TechCrunch (3 emails)\n- Apple announced new M5 chips...\n\n## ShopDaily (2 emails)\n- Spring sale ending Friday...",
  "sources": [
    {
      "sender": "newsletter@techcrunch.com",
      "count": 3,
      "subjects": ["Apple M5 Launch", "AI Roundup", "Startup Funding"]
    }
  ],
  "generated_at": 1710300000
}
```

Returns `503` if AI is disabled. Returns an empty digest (no AI call) when no newsletters match.

---

**GET** `/api/ai/newsletter-digest/sources`

Lists all detected newsletter senders with email counts, grouped by `from_address`. Limited to top 100 sources by count.

**Response:**
```json
{
  "sources": [
    {
      "sender": "newsletter@example.com",
      "sender_name": "Example Newsletter",
      "email_count": 34,
      "last_received": 1710200000,
      "categories": ["Newsletters"]
    }
  ]
}
```

No AI required.

---

**POST** `/api/ai/newsletter-digest/preview`

Previews which messages would be included in a digest without generating the summary.

**Request:** Same shape as `DigestRequest`.

**Response:**
```json
{
  "messages": [
    {
      "id": "msg-abc123",
      "from": "newsletter@example.com",
      "subject": "Weekly Update",
      "date": 1710200000
    }
  ],
  "total_count": 12
}
```

No AI required.

---

**GET** `/api/ai/newsletter-digest/history?limit={n}&offset={n}`

Lists previously generated digests, newest first.

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `limit` | int | 20 | Max results (capped at 100) |
| `offset` | int | 0 | Pagination offset |

**Response:**
```json
{
  "digests": [
    {
      "id": "digest-abc123",
      "title": "Weekly Tech & Commerce Digest",
      "summary": "## TechCrunch...",
      "source_count": 4,
      "message_count": 12,
      "date_from": 1709000000,
      "date_to": 1710000000,
      "created_at": 1710300000
    }
  ]
}
```

### Database

**Table:** `newsletter_digests` (migration 035)

| Column | Type | Description |
|--------|------|-------------|
| `id` | TEXT PK | UUID |
| `title` | TEXT | AI-generated or default title |
| `summary` | TEXT | AI-generated digest content |
| `message_ids` | TEXT | JSON array of source message IDs |
| `source_count` | INTEGER | Number of unique senders |
| `message_count` | INTEGER | Total emails included |
| `date_from` | INTEGER | Filter start (nullable) |
| `date_to` | INTEGER | Filter end (nullable) |
| `created_at` | INTEGER | Generation timestamp |

### How It Works

Newsletter detection uses a combined SQL condition: `raw_headers LIKE '%List-Unsubscribe%' OR ai_category IN ('Newsletters', 'Promotions')`. Matched emails are grouped into a `BTreeMap` by sender, then a prompt is constructed with each sender's emails (subject + snippet + date). The AI system prompt instructs the model to output a `TITLE: ` prefix line followed by the digest body. The parser splits on the first newline after `TITLE: ` to extract the title and summary separately.

Each generated digest is stored in `newsletter_digests` with the source message IDs for auditability.

### Configuration

Requires AI enabled and at least one configured provider for the `generate` endpoint. Sources, preview, and history endpoints work without AI.

---

## #69 Template Auto-Generation

Scans sent emails from the last 90 days, groups them by recipient and normalized subject similarity, and uses AI to extract reusable templates with `{{variable}}` placeholders. Users can accept suggestions (which creates a real template in the `templates` table) or dismiss them.

### API

**POST** `/api/ai/template-suggestions/scan`

Scans recent sent emails, groups similar ones (minimum 3 per group), and generates template suggestions via AI. Skips groups below 0.5 confidence.

**Response:**
```json
{
  "scanned": 142,
  "suggestions_created": 3
}
```

Returns `503` if AI is disabled.

---

**GET** `/api/ai/template-suggestions?status={s}&min_confidence={n}`

Lists template suggestions with optional filtering.

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `status` | string | `pending` | `pending`, `accepted`, `dismissed`, or `all` |
| `min_confidence` | float | 0.0 | Minimum confidence threshold |

**Response:**
```json
[
  {
    "id": "sug-abc123",
    "name": "Weekly Status Update",
    "subject_pattern": "Status Update - {{week}}",
    "body_pattern": "Hi team,\n\nHere is this week's update:\n\n{{content}}\n\nBest,\n{{name}}",
    "sample_message_ids": ["msg-1", "msg-2", "msg-3"],
    "pattern_count": 5,
    "confidence": 0.87,
    "status": "pending",
    "created_at": 1710288000,
    "accepted_at": null,
    "dismissed_at": null
  }
]
```

---

**POST** `/api/ai/template-suggestions/{id}/accept`

Converts a pending suggestion into a real template. Creates a row in `templates` and marks the suggestion as `accepted`.

**Response:**
```json
{
  "template": {
    "id": "tpl-abc123",
    "account_id": "acc-123",
    "name": "Weekly Status Update",
    "subject": "Status Update - {{week}}",
    "body": "Hi team,\n\nHere is this week's update:\n\n{{content}}\n\nBest,\n{{name}}",
    "created_at": 1710300000,
    "updated_at": 1710300000
  }
}
```

Returns `404` if suggestion not found, `409` if already accepted or dismissed.

---

**DELETE** `/api/ai/template-suggestions/{id}`

Dismisses a pending suggestion. Sets `status = 'dismissed'` and records `dismissed_at`.

**Response:**
```json
{
  "dismissed": true
}
```

Returns `404` if not found or not in `pending` status.

### Database

**Table:** `template_suggestions` (migration 036)

| Column | Type | Description |
|--------|------|-------------|
| `id` | TEXT PK | UUID |
| `name` | TEXT | AI-generated template name |
| `subject_pattern` | TEXT | Subject with `{{variables}}` (nullable) |
| `body_pattern` | TEXT | Body with `{{variables}}` |
| `sample_message_ids` | TEXT | JSON array of source message IDs |
| `pattern_count` | INTEGER | Number of similar emails found |
| `confidence` | REAL | AI-assessed pattern confidence (0.0-1.0) |
| `status` | TEXT | `pending`, `accepted`, or `dismissed` |

**Table:** `templates` (migration 036)

| Column | Type | Description |
|--------|------|-------------|
| `id` | TEXT PK | UUID |
| `account_id` | TEXT | Owner account |
| `name` | TEXT | Template name |
| `subject` | TEXT | Subject line (nullable) |
| `body` | TEXT | Template body |

### How It Works

The scan endpoint queries sent emails (folders `Sent` or `[Gmail]/Sent Mail`) from the last 90 days with non-empty bodies, up to 500 messages. These are grouped by normalized subject similarity via the `group_similar_emails` helper. For each group with 3+ messages, up to 5 samples are sent to the AI provider with a system prompt that instructs it to identify fixed structure vs variable parts and output JSON with `{{variable_name}}` placeholders and a confidence score.

Suggestions with confidence < 0.5 are discarded. Existing pending suggestions with the same name are updated (upsert behavior) rather than duplicated. Accept creates a new `templates` row using the suggestion's name, subject_pattern, and body_pattern.

### Integration

- Accept creates templates that integrate with the existing templates system (migration 036 creates the `templates` table).
- Scans use the Sent folder, so the feature works regardless of AI classification status on received messages.

### Configuration

Requires AI enabled for the scan endpoint. List, accept, and dismiss are pure database operations.

---

## #70 Smart Notification Routing

Routes incoming email notifications based on a priority cascade: VIP sender > intent > urgency threshold > category > default. Supports three routes (push, digest, silent), quiet hours that downgrade non-urgent push to digest, and a digest queue for batched delivery.

### API

**GET** `/api/notifications/routing/config`

Returns the current routing configuration.

**Response:**
```json
{
  "push_categories": ["Work", "Finance"],
  "digest_categories": ["Newsletters", "Social"],
  "silent_categories": ["Promotions", "Spam"],
  "push_senders": ["boss@company.com"],
  "digest_interval_minutes": 60,
  "quiet_hours_start": "22:00",
  "quiet_hours_end": "07:00",
  "vip_always_push": true,
  "urgency_threshold": "high"
}
```

---

**PUT** `/api/notifications/routing/config`

Updates the full routing configuration. Validates all fields.

**Request:** Same shape as the GET response.

**Validation:**
- `digest_interval_minutes` must be > 0
- `urgency_threshold` must be one of: `low`, `normal`, `high`, `urgent`
- `quiet_hours_start` / `quiet_hours_end` must be valid `HH:MM` format if provided

Returns `400` on validation failure.

---

**POST** `/api/notifications/routing/classify`

Classifies a single message and stores the result in the digest queue.

**Request:**
```json
{
  "message_id": "msg-abc123"
}
```

**Response:**
```json
{
  "route": "push",
  "reason": "sender is VIP contact",
  "message_id": "msg-abc123"
}
```

`route` values: `"push"`, `"digest"`, `"silent"`.

Returns `404` if message not found.

---

**GET** `/api/notifications/digest`

Returns all unread digest items, grouped with category counts.

**Response:**
```json
{
  "items": [
    {
      "id": "item-abc123",
      "message_id": "msg-abc123",
      "account_id": "acc-123",
      "from_address": "news@example.com",
      "subject": "Weekly roundup",
      "category": "Newsletters",
      "priority": "normal",
      "route": "digest",
      "created_at": 1710288000
    }
  ],
  "total": 5,
  "categories": {
    "Newsletters": 3,
    "Social": 2
  }
}
```

---

**POST** `/api/notifications/digest/clear`

Marks all unread digest items as read.

**Response:**
```json
{
  "cleared": 5
}
```

### Priority Cascade

The classification engine evaluates in order, returning the first match:

| Priority | Check | Route |
|----------|-------|-------|
| 1 | Sender in `push_senders` list or `vip_contacts` table (if `vip_always_push` enabled) | push |
| 2 | `ai_intent` is `action_required` or `action_request` | push |
| 3 | `ai_priority_label` meets or exceeds `urgency_threshold` | push |
| 4 | `ai_category` in `push_categories` | push |
| 5 | `ai_category` in `silent_categories` | silent |
| 6 | `ai_category` in `digest_categories` | digest |
| 7 | Default | digest |

After classification, quiet hours check runs: if route is `push` and current time is within `quiet_hours_start`-`quiet_hours_end`, and the message is not urgency `urgent`, the route is downgraded to `digest`. Overnight ranges (e.g., 22:00-07:00) are handled correctly.

### Database

**Table:** `notification_routing_config` (migration 037)

Singleton row (`id = 1`). Stores category arrays as JSON text, quiet hours as `HH:MM` strings.

**Table:** `notification_digest_items` (migration 037)

| Column | Type | Description |
|--------|------|-------------|
| `id` | TEXT PK | UUID |
| `message_id` | TEXT | Source message |
| `account_id` | TEXT | Account |
| `from_address` | TEXT | Sender (nullable) |
| `subject` | TEXT | Subject (nullable) |
| `category` | TEXT | AI category (nullable) |
| `priority` | TEXT | AI priority label (nullable) |
| `route` | TEXT | Classified route |
| `is_read` | INTEGER | 0 = unread, 1 = cleared |

### Integration

- Reads AI classification fields (`ai_category`, `ai_priority_label`, `ai_intent`) from the V6 AI pipeline.
- Checks the `vip_contacts` table from feature #84 (VIP management) for VIP sender lookup.
- Urgency ranking: low=0, normal=1, high=2, urgent=3. Threshold comparison uses `>=`.

### Configuration

Default config (migration 037 inserts it): empty category arrays, 60-minute digest interval, no quiet hours, VIP always push enabled, urgency threshold `high`. All configurable via the PUT endpoint.

---

## #86 Follow-up If No Reply

Tracks sent emails that expect replies. Users create a follow-up tracker on a sent message with a configurable deadline (1-90 days). The system can scan for replies in the thread and auto-resolve trackers when a reply is detected. Surfaces overdue follow-ups with computed `days_remaining` and `is_overdue` fields.

### API

**POST** `/api/followup-tracking`

Creates a follow-up tracker for a sent message.

**Request:**
```json
{
  "message_id": "msg-abc123",
  "days": 3,
  "note": "Need budget numbers by Friday"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `message_id` | string | yes | Must be in the Sent folder |
| `days` | int | yes | Days until follow-up due (1-90) |
| `note` | string | no | Optional reminder note |

**Response:**
```json
{
  "id": "ft-abc123",
  "message_id": "msg-abc123",
  "account_id": "acc-123",
  "thread_id": "thread-456",
  "to_address": "alice@example.com",
  "subject": "Q3 Budget Request",
  "sent_at": 1710200000,
  "followup_after": 1710459200,
  "status": "active",
  "note": "Need budget numbers by Friday",
  "reply_message_id": null,
  "reply_detected_at": null,
  "created_at": 1710300000,
  "updated_at": 1710300000,
  "days_remaining": 2,
  "is_overdue": false
}
```

Returns `400` if message is not in Sent folder, days out of range, or recipient is empty. Returns `404` if message not found. Returns `409` on duplicate (same message + recipient).

---

**GET** `/api/followup-tracking?status={s}`

Lists all trackers, optionally filtered by status. Ordered by `followup_after` ascending (soonest due first).

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `status` | string | all | `active`, `replied`, `followed_up`, or `cancelled` |

---

**GET** `/api/followup-tracking/due`

Returns only active trackers that are past their `followup_after` deadline (overdue items).

---

**PUT** `/api/followup-tracking/{id}`

Updates an active tracker's deadline or note.

**Request:**
```json
{
  "days": 7,
  "note": "Extended deadline"
}
```

Recalculates `followup_after` as `sent_at + (days * 86400)`. Only active trackers can be updated. Returns `400` if tracker is not active or days out of range.

---

**DELETE** `/api/followup-tracking/{id}`

Cancels an active tracker (sets `status = 'cancelled'`). Does not delete the row.

**Response:**
```json
{
  "cancelled": true
}
```

Returns `404` if not found or not active.

---

**POST** `/api/followup-tracking/check-replies`

Scans all active trackers for thread replies. For each tracker, checks if any message in the same thread has `from_address` matching the tracked `to_address` and was sent after the original message. When found, updates the tracker to `status = 'replied'` with the reply message ID and detection timestamp.

**Response:**
```json
{
  "checked": 12,
  "replies_found": 3
}
```

### Database

**Table:** `followup_tracking` (migration 038)

| Column | Type | Description |
|--------|------|-------------|
| `id` | TEXT PK | UUID |
| `message_id` | TEXT | Source sent message |
| `account_id` | TEXT | Account |
| `thread_id` | TEXT | Thread ID for reply detection (nullable) |
| `to_address` | TEXT | Recipient being tracked |
| `subject` | TEXT | Email subject (nullable) |
| `sent_at` | INTEGER | When the email was sent |
| `followup_after` | INTEGER | Deadline epoch (`sent_at + days * 86400`) |
| `status` | TEXT | `active`, `replied`, `followed_up`, `cancelled` |
| `note` | TEXT | User reminder note (nullable) |
| `reply_message_id` | TEXT | ID of detected reply (nullable) |
| `reply_detected_at` | INTEGER | When reply was detected (nullable) |

Unique constraint on `(message_id, to_address)`. Indexes on `status` and `followup_after WHERE status = 'active'`.

### How It Works

Creation validates the message is in the Sent folder, extracts the first recipient from the `to_addresses` JSON array, and computes `followup_after = sent_at + (days * 86400)`. The `days_remaining` and `is_overdue` fields are computed at read time from `followup_after - now()`.

Reply checking queries `messages WHERE thread_id = ? AND LOWER(from_address) = LOWER(to_address) AND date > sent_at`, case-insensitive. On match, the tracker is resolved to `replied` status with the reply message ID recorded.

Delete is a soft cancel (status change) rather than row deletion, preserving history.

### Integration

- Uses thread IDs from the V2 email reader for reply detection.
- The `to_addresses` field is parsed as a JSON array (same format used throughout Iris).
- Folder name check uses exact string match on `"Sent"` (does not check `[Gmail]/Sent Mail` variant for creation validation).

### Configuration

No configuration. No AI required.

---

## #87 Email Effectiveness Scoring

AI-powered before-send scoring that evaluates email drafts across 5 dimensions: clarity, tone, length, subject line, and call-to-action. Each dimension produces a 0-1 score plus written feedback. An overall weighted score is computed and stored for historical tracking. A separate tips endpoint provides standalone improvement suggestions.

### API

**POST** `/api/compose/effectiveness-score`

Scores an email draft and stores the result.

**Request:**
```json
{
  "account_id": "acc-123",
  "subject": "Q3 Budget Review",
  "body": "Hi team,\n\nPlease review the attached budget spreadsheet and provide feedback by Friday.\n\nBest,\nAlice",
  "to": "team@example.com",
  "context": "formal business email",
  "draft_id": "draft-456"
}
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `account_id` | string | yes | Account ID |
| `body` | string | yes | Email body (must be non-empty, max 100KB) |
| `subject` | string | no | Subject line (defaults to "(no subject)" in prompt) |
| `to` | string | no | Recipient context |
| `context` | string | no | Additional context for scoring |
| `draft_id` | string | no | Links score to a specific draft |

**Response:**
```json
{
  "id": "score-abc123",
  "overall_score": 0.78,
  "breakdown": {
    "clarity": 0.85,
    "tone": 0.80,
    "length": 0.90,
    "subject_line": 0.60,
    "call_to_action": 0.70
  },
  "feedback": {
    "clarity": "Message is clear and well-structured.",
    "tone": "Appropriate professional tone.",
    "length": "Good length for this type of email.",
    "subject_line": "Subject could be more specific about the action needed.",
    "call_to_action": "Clear deadline but could specify the exact feedback format desired."
  },
  "tips": [
    "Make the subject more specific, e.g. 'Q3 Budget Review - Feedback Needed by Friday'",
    "Specify what kind of feedback you're looking for"
  ]
}
```

Returns `400` if body is empty/whitespace, `413` if body exceeds 100KB, `503` if AI is disabled, `502` if AI returns no result.

---

**GET** `/api/compose/effectiveness-history?account_id={id}&limit={n}`

Returns past scores for historical tracking.

| Param | Type | Default | Description |
|-------|------|---------|-------------|
| `account_id` | string | all | Filter by account |
| `limit` | int | 10 | Max results (clamped 1-100) |

**Response:**
```json
{
  "scores": [
    {
      "id": "score-abc123",
      "subject": "Q3 Budget Review",
      "overall_score": 0.78,
      "breakdown": {
        "clarity": 0.85,
        "tone": 0.80,
        "length": 0.90,
        "subject_line": 0.60,
        "call_to_action": 0.70
      },
      "created_at": 1710300000
    }
  ],
  "average_overall": 0.78
}
```

`average_overall` is the mean of all returned scores, rounded to 2 decimal places. Returns `0.0` if no history exists.

---

**POST** `/api/compose/effectiveness-tips`

Standalone tips generation without scoring. Returns 1-5 actionable improvement tips.

**Request:**
```json
{
  "subject": "Meeting Tomorrow",
  "body": "Can we meet tomorrow?"
}
```

**Response:**
```json
{
  "tips": [
    "Specify a time and location for the meeting",
    "Add context about the meeting topic",
    "Include an alternative time in case tomorrow doesn't work"
  ]
}
```

Returns `400` if body is empty, `413` if over 100KB, `503` if AI disabled.

### Scoring Dimensions

| Dimension | Weight | What It Measures |
|-----------|--------|-----------------|
| **Clarity** | 25% | Structure, conciseness, lack of ambiguity |
| **Tone** | 20% | Appropriateness for the context (professional vs casual) |
| **Length** | 15% | Whether length matches the email's purpose |
| **Subject Line** | 20% | Effectiveness and descriptiveness (0.5 default if missing) |
| **Call to Action** | 20% | Presence and clarity of next steps |

Overall score formula: `clarity * 0.25 + tone * 0.20 + length * 0.15 + subject_line * 0.20 + call_to_action * 0.20`, rounded to 2 decimal places.

### Database

**Table:** `effectiveness_scores` (migration 039)

| Column | Type | Description |
|--------|------|-------------|
| `id` | TEXT PK | UUID |
| `account_id` | TEXT | Account |
| `draft_id` | TEXT | Linked draft (nullable) |
| `subject` | TEXT | Subject at time of scoring (nullable) |
| `overall_score` | REAL | Weighted composite score |
| `clarity_score` | REAL | Clarity dimension (0-1) |
| `tone_score` | REAL | Tone dimension (0-1) |
| `length_score` | REAL | Length dimension (0-1) |
| `subject_score` | REAL | Subject line dimension (0-1) |
| `cta_score` | REAL | Call-to-action dimension (0-1) |
| `feedback` | TEXT | JSON object with per-dimension feedback strings |
| `tips` | TEXT | JSON array of improvement tips |

### How It Works

The scoring endpoint builds a user prompt containing the subject, recipient, optional context, and body, then sends it to the AI provider with a system prompt that requests strict JSON output with 5 dimension scores (0-1), per-dimension feedback, and 1-3 tips. The `parse_score_response` function handles AI responses wrapped in markdown code fences by stripping them before JSON parsing. Missing dimensions default to 0.5, out-of-range scores are clamped to [0.0, 1.0].

The tips endpoint uses a separate, simpler system prompt that asks for a JSON array of 1-5 improvement strings. Fallback parsing handles both `["tip"]` arrays and `{"tips": ["tip"]}` objects. If parsing fails entirely, a generic default tip is returned.

### Integration

- Designed to be called from the compose modal before sending (pre-send quality check).
- The `draft_id` field links scores back to specific drafts for tracking improvements across revisions.
- Uses the same `ProviderPool::generate` as all other AI features (Ollama, Anthropic, OpenAI).

### Configuration

Requires AI enabled (`ai_enabled = true` in config) and at least one configured provider. Tips and scoring endpoints share the same AI requirement. History is a pure database query.
