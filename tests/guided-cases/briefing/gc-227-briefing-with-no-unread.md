# GC-227: Briefing — Works with No Unread Messages

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: briefing
- **Tags**: briefing, api, edge, empty, no-unread
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000
- At least one AI provider configured and healthy

### Data
- All messages in the inbox are marked as read (no unread messages)
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Ensure all messages are marked read
   - **Target**: `PUT /api/messages/batch`
   - **Input**: Mark all messages as read (or verify all are already read)
   - **Expected**: 200 OK

2. Request a daily briefing
   - **Target**: `GET /api/ai/briefing`
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: 200 OK with valid response; `stats.unread` is 0; summary reflects the quiet inbox

3. Verify the response is still well-formed
   - **Target**: Response body
   - **Input**: n/a
   - **Expected**: `summary`, `stats`, and `highlights` are all present; `highlights` may be empty

## Success Criteria
- [ ] Response status is 200 (not 404 or 204)
- [ ] `stats.unread` is 0
- [ ] `summary` is still a meaningful string (e.g., "Your inbox is clear" or similar)
- [ ] `highlights` is an empty array or contains only non-urgent items

## Failure Criteria
- Server returns an error when there are no unread messages
- `summary` is empty or null
- `stats` values are incorrect (e.g., showing unread > 0 when all are read)
