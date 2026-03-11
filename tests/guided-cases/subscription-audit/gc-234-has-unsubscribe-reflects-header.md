# GC-234: Subscription Audit — has_unsubscribe Matches List-Unsubscribe Header

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: subscription-audit
- **Tags**: subscription-audit, api, unsubscribe, header
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000

### Data
- At least one recurring sender whose emails include the `List-Unsubscribe` header
- At least one recurring sender whose emails do not include the header
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Request the subscription audit
   - **Target**: `GET /api/ai/subscription-audit`
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: 200 OK with a `subscriptions` array

2. Find a subscription from a sender known to have List-Unsubscribe
   - **Target**: Subscription object for the known sender
   - **Input**: n/a
   - **Expected**: `has_unsubscribe` is `true`

3. Find a subscription from a sender known to lack List-Unsubscribe
   - **Target**: Subscription object for the sender without the header
   - **Input**: n/a
   - **Expected**: `has_unsubscribe` is `false`

## Success Criteria
- [ ] `has_unsubscribe` is `true` for senders whose emails contain the List-Unsubscribe header
- [ ] `has_unsubscribe` is `false` for senders whose emails lack the header
- [ ] `has_unsubscribe` is a boolean (not a string or null)

## Failure Criteria
- `has_unsubscribe` does not match the actual header presence
- `has_unsubscribe` is always `true` or always `false` regardless of headers
- Field is a non-boolean type
