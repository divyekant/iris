# GC-235: Subscription Audit — last_received Is a Valid Timestamp

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: subscription-audit
- **Tags**: subscription-audit, api, timestamp, validation
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000

### Data
- At least one email account synced with recurring senders
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Request the subscription audit
   - **Target**: `GET /api/ai/subscription-audit`
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: 200 OK with a non-empty `subscriptions` array

2. Verify last_received is a valid timestamp on each subscription
   - **Target**: `last_received` field in each subscription object
   - **Input**: n/a
   - **Expected**: `last_received` is either a Unix timestamp (positive integer) or an ISO 8601 date string; the value is in the past (not a future date)

3. Verify ordering plausibility
   - **Target**: The `last_received` value for a known recent sender
   - **Input**: n/a
   - **Expected**: The timestamp corresponds to a recent date (within the last few months)

## Success Criteria
- [ ] Every `last_received` is a valid timestamp (integer or ISO string)
- [ ] No `last_received` value is in the future
- [ ] Timestamps are plausible (not epoch 0 or year 1970 for recent emails)

## Failure Criteria
- `last_received` is null, empty, or missing
- Timestamp is invalid (e.g., negative, or a malformed string)
- All timestamps are identical (suggesting a bug)
