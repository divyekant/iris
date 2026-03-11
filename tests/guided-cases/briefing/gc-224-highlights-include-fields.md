# GC-224: Briefing — Highlights Include message_id, from, subject, reason

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: briefing
- **Tags**: briefing, api, highlights, fields, schema
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000
- At least one AI provider configured and healthy

### Data
- At least one email account synced with recent messages (so highlights are generated)
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Request a daily briefing
   - **Target**: `GET /api/ai/briefing`
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: 200 OK with a non-empty `highlights` array

2. Inspect each highlight object
   - **Target**: Each item in the `highlights` array
   - **Input**: n/a
   - **Expected**: Each highlight has:
     - `message_id`: non-empty string (ID of the highlighted message)
     - `from`: non-empty string (sender address or name)
     - `subject`: string (email subject line)
     - `reason`: non-empty string (why this message is highlighted)

## Success Criteria
- [ ] Each highlight has a `message_id` field (non-empty string)
- [ ] Each highlight has a `from` field (non-empty string)
- [ ] Each highlight has a `subject` field (string, may be empty for no-subject emails)
- [ ] Each highlight has a `reason` field (non-empty string explaining relevance)

## Failure Criteria
- Any highlight is missing `message_id`, `from`, `subject`, or `reason`
- `message_id` is empty or null
- `reason` is empty or generic (e.g., just "important")
