# GC-222: Briefing — Highlights Array Present

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: briefing
- **Tags**: briefing, api, highlights, schema
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
   - **Expected**: 200 OK with a `highlights` array in the response

2. Verify highlights array structure
   - **Target**: `highlights` field in the response
   - **Input**: n/a
   - **Expected**: `highlights` is an array; each element is an object representing a noteworthy message

## Success Criteria
- [ ] `highlights` is present and is an array
- [ ] Array elements are objects (not primitives)
- [ ] When messages exist, at least one highlight is returned

## Failure Criteria
- `highlights` is missing from the response
- `highlights` is null instead of an array
- `highlights` contains primitive values instead of objects
