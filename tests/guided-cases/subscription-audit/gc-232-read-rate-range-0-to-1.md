# GC-232: Subscription Audit — read_rate in Range 0 to 1

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: subscription-audit
- **Tags**: subscription-audit, api, read-rate, validation
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000

### Data
- At least one email account synced with recurring senders (some read, some unread)
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Request the subscription audit
   - **Target**: `GET /api/ai/subscription-audit`
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: 200 OK with a non-empty `subscriptions` array

2. Verify read_rate is within bounds for every subscription
   - **Target**: `read_rate` field in each subscription object
   - **Input**: n/a
   - **Expected**: Every `read_rate` value is a number >= 0.0 and <= 1.0

3. Verify read_rate semantics
   - **Target**: A subscription where some emails were read and some were not
   - **Input**: n/a
   - **Expected**: `read_rate` is between 0 and 1 (exclusive), reflecting partial read status

## Success Criteria
- [ ] Every `read_rate` is a number in the range [0.0, 1.0]
- [ ] `read_rate` is not a percentage (e.g., not 75 instead of 0.75)
- [ ] Subscriptions with all-read emails have `read_rate` close to 1.0
- [ ] Subscriptions with all-unread emails have `read_rate` of 0.0

## Failure Criteria
- `read_rate` is outside the range [0.0, 1.0]
- `read_rate` is expressed as a percentage (0-100 instead of 0-1)
- `read_rate` is a string instead of a number
