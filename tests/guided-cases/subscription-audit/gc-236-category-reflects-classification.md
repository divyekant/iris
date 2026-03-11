# GC-236: Subscription Audit — Category Reflects AI Classification

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: subscription-audit
- **Tags**: subscription-audit, api, category, ai-classification
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000

### Data
- At least one email account synced with messages that have been AI-classified
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Request the subscription audit
   - **Target**: `GET /api/ai/subscription-audit`
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: 200 OK with a non-empty `subscriptions` array

2. Verify category field on each subscription
   - **Target**: `category` field in each subscription object
   - **Input**: n/a
   - **Expected**: `category` is a non-empty string reflecting the AI-assigned category (e.g., "primary", "promotions", "social", "updates")

3. Cross-check with message categories
   - **Target**: Messages from a known subscription sender via `GET /api/messages?account_id={id}&folder=INBOX`
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: The subscription's `category` matches the most common `ai_category` among that sender's messages

## Success Criteria
- [ ] Every subscription has a `category` field
- [ ] `category` is a non-empty string
- [ ] Category values align with the AI classification system (known categories)
- [ ] Category is consistent with the sender's email classification

## Failure Criteria
- `category` is missing, null, or empty
- Category values are unrecognized (outside the known set)
- Category contradicts the AI classification of the sender's messages
