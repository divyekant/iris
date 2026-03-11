# GC-229: Subscription Audit — Happy Path

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: subscription-audit
- **Tags**: subscription-audit, api, happy-path
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000

### Data
- At least one email account synced with recurring sender emails (e.g., newsletters, promotions)
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Request the subscription audit
   - **Target**: `GET /api/ai/subscription-audit`
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: 200 OK with JSON body containing a `subscriptions` array

2. Verify the response structure
   - **Target**: Response body
   - **Input**: n/a
   - **Expected**: `subscriptions` is an array of subscription objects; each object describes a recurring sender

## Success Criteria
- [ ] Response status is 200
- [ ] `subscriptions` is present and is an array
- [ ] Array contains at least one subscription object (given recurring senders exist)
- [ ] Each object is a well-formed subscription descriptor

## Failure Criteria
- Response status is not 200
- `subscriptions` is missing or null
- Array is empty despite recurring senders in the inbox
