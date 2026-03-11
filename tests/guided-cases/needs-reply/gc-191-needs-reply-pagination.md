# GC-191: Needs-Reply Pagination with Limit/Offset

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: needs-reply
- **Tags**: needs-reply, api, pagination, limit, offset
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000

### Data
- At least 3 messages flagged as needs-reply
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Fetch the first page with limit=2
   - **Target**: `GET /api/messages/needs-reply?limit=2&offset=0`
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: 200 OK; `messages` array has at most 2 items; `total` reflects the full count

2. Fetch the second page with limit=2, offset=2
   - **Target**: `GET /api/messages/needs-reply?limit=2&offset=2`
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: 200 OK; `messages` array contains the next batch; no overlap with first page

3. Verify no overlap between pages
   - **Target**: Compare message IDs from step 1 and step 2
   - **Input**: n/a
   - **Expected**: No message ID appears in both pages

## Success Criteria
- [ ] First page returns at most 2 messages
- [ ] Second page returns different messages than the first
- [ ] `total` is consistent across both requests
- [ ] No message ID overlap between pages

## Failure Criteria
- More than `limit` messages returned in a single page
- Duplicate messages across pages
- `total` differs between paginated requests
