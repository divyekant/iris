# GC-238: Subscription Audit — Empty Inbox Returns Empty List

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: subscription-audit
- **Tags**: subscription-audit, api, edge, empty
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000

### Data
- Fresh database with no messages or an account with no recurring senders meeting the minimum threshold
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Request the subscription audit on an empty or minimal inbox
   - **Target**: `GET /api/ai/subscription-audit`
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: 200 OK with `{"subscriptions": []}`

2. Verify the response is well-formed
   - **Target**: Response body
   - **Input**: n/a
   - **Expected**: `subscriptions` is an empty array `[]`; response is not null or an error

## Success Criteria
- [ ] Response status is 200 (not 404 or 204)
- [ ] `subscriptions` is an empty array `[]`
- [ ] No error message is returned

## Failure Criteria
- Response status is 404, 204, or 500
- `subscriptions` is `null` instead of an empty array
- Server crashes on empty data
