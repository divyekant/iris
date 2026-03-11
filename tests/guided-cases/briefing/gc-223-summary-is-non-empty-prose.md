# GC-223: Briefing — Summary Is Non-Empty Meaningful Text

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: briefing
- **Tags**: briefing, api, summary, quality
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000
- At least one AI provider configured and healthy

### Data
- At least one email account synced with messages
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Request a daily briefing
   - **Target**: `GET /api/ai/briefing`
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: 200 OK with a `summary` string

2. Evaluate the summary content
   - **Target**: `summary` field in the response
   - **Input**: n/a
   - **Expected**: `summary` is a non-empty string with at least 20 characters; reads as meaningful prose (not a single word, not JSON, not gibberish)

## Success Criteria
- [ ] `summary` is a non-empty string
- [ ] `summary` has at least 20 characters
- [ ] `summary` reads as coherent human-readable text
- [ ] `summary` relates to the user's inbox state (mentions counts, topics, or priorities)

## Failure Criteria
- `summary` is empty, null, or missing
- `summary` is a single word or very short generic string (e.g., "OK")
- `summary` contains JSON artifacts or raw data structures
