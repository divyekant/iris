# GC-219: Briefing — Happy Path

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: briefing
- **Tags**: briefing, api, happy-path, ai
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
   - **Expected**: 200 OK with JSON body containing `summary`, `stats`, and `highlights` fields

2. Verify the top-level response structure
   - **Target**: Response body
   - **Input**: n/a
   - **Expected**: `summary` is a non-empty string; `stats` is an object; `highlights` is an array

## Success Criteria
- [ ] Response status is 200
- [ ] `summary` is a non-empty string
- [ ] `stats` is a JSON object with numeric fields
- [ ] `highlights` is an array

## Failure Criteria
- Response status is not 200
- Any of `summary`, `stats`, or `highlights` is missing
- `summary` is empty
