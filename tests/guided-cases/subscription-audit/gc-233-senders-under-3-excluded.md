# GC-233: Subscription Audit — Senders with Fewer Than 3 Emails Excluded

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: subscription-audit
- **Tags**: subscription-audit, api, filter, minimum-count, edge
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000

### Data
- At least one sender with 3+ emails in the inbox
- At least one sender with fewer than 3 emails
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Request the subscription audit
   - **Target**: `GET /api/ai/subscription-audit`
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: 200 OK with a `subscriptions` array

2. Verify minimum email count filter
   - **Target**: `count` field on each subscription object
   - **Input**: n/a
   - **Expected**: Every subscription has `count >= 3`; senders with only 1-2 emails are excluded

3. Cross-check against the message list
   - **Target**: `GET /api/messages?account_id={id}&folder=INBOX`
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: Senders with < 3 emails in the inbox are absent from the subscription audit results

## Success Criteria
- [ ] Every subscription in the list has `count >= 3`
- [ ] No subscription has `count` of 1 or 2
- [ ] One-off or two-time senders do not appear

## Failure Criteria
- A subscription with `count < 3` appears in the results
- The filter threshold is not applied consistently
