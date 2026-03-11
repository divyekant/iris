# GC-225: Briefing — Stats Values Are Integers

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: briefing
- **Tags**: briefing, api, stats, types, validation
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000
- At least one AI provider configured and healthy

### Data
- At least one email account synced
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Request a daily briefing
   - **Target**: `GET /api/ai/briefing`
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: 200 OK with a `stats` object

2. Verify all stats values are numeric integers
   - **Target**: `stats.total_today`, `stats.unread`, `stats.needs_reply`, `stats.urgent`
   - **Input**: n/a
   - **Expected**: All four values are integers (not strings, not floats, not null); all are >= 0

3. Cross-check stats against actual data
   - **Target**: Compare `stats.unread` with `GET /api/messages?account_id={id}&folder=INBOX` unread_count
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: `stats.unread` is consistent with the actual unread count from the message list

## Success Criteria
- [ ] `total_today` is a non-negative integer
- [ ] `unread` is a non-negative integer
- [ ] `needs_reply` is a non-negative integer
- [ ] `urgent` is a non-negative integer
- [ ] Values are consistent with actual inbox state

## Failure Criteria
- Any stat value is a string, float, or null
- Any stat value is negative
- Stats are wildly inconsistent with actual inbox data
