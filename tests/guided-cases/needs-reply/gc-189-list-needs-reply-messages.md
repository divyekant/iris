# GC-189: List Needs-Reply Messages Endpoint Returns 200

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: needs-reply
- **Tags**: needs-reply, api, happy-path, list
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000

### Data
- At least one email account synced with messages processed by the AI pipeline
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Fetch the needs-reply message list
   - **Target**: `GET /api/messages/needs-reply`
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: 200 OK with JSON body `{"messages": [...], "total": N}`

2. Inspect the response structure
   - **Target**: Response body
   - **Input**: n/a
   - **Expected**: `messages` is an array; `total` is a non-negative integer matching the array length (or total count when paginated)

## Success Criteria
- [ ] Response status is 200
- [ ] Response body contains a `messages` array
- [ ] Response body contains a `total` integer field
- [ ] Array items are valid message summary objects

## Failure Criteria
- Response status is not 200
- Response body is missing `messages` or `total` fields
- `messages` is not an array
