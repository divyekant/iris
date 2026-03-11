# GC-220: Briefing — Stats Object Has Required Fields

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: briefing
- **Tags**: briefing, api, stats, schema
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
   - **Expected**: 200 OK with a `stats` object in the response

2. Inspect the stats object
   - **Target**: `stats` field in the response
   - **Input**: n/a
   - **Expected**: `stats` contains the following keys:
     - `total_today`: integer (total messages received today)
     - `unread`: integer (unread message count)
     - `needs_reply`: integer (messages flagged as needing reply)
     - `urgent`: integer (urgent/high-priority messages)

## Success Criteria
- [ ] `stats` object contains `total_today` as an integer
- [ ] `stats` object contains `unread` as an integer
- [ ] `stats` object contains `needs_reply` as an integer
- [ ] `stats` object contains `urgent` as an integer
- [ ] All values are non-negative

## Failure Criteria
- Any of the four keys is missing from `stats`
- Values are not integers (e.g., strings, floats, null)
- Values are negative
