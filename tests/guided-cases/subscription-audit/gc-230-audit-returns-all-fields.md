# GC-230: Subscription Audit — Returns All Required Fields

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: subscription-audit
- **Tags**: subscription-audit, api, schema, fields
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000

### Data
- At least one email account synced with recurring sender emails
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Request the subscription audit
   - **Target**: `GET /api/ai/subscription-audit`
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: 200 OK with a non-empty `subscriptions` array

2. Inspect each subscription object for required fields
   - **Target**: Each item in the `subscriptions` array
   - **Input**: n/a
   - **Expected**: Each subscription has:
     - `sender`: string (sender email or domain)
     - `count`: integer (number of emails from this sender)
     - `read_rate`: number (0 to 1, proportion of emails read)
     - `has_unsubscribe`: boolean (whether List-Unsubscribe header was present)
     - `last_received`: string or integer (timestamp of most recent email)
     - `category`: string (AI-classified category, e.g., "promotions", "social")

## Success Criteria
- [ ] Each subscription has `sender` (non-empty string)
- [ ] Each subscription has `count` (positive integer)
- [ ] Each subscription has `read_rate` (number)
- [ ] Each subscription has `has_unsubscribe` (boolean)
- [ ] Each subscription has `last_received` (timestamp)
- [ ] Each subscription has `category` (string)

## Failure Criteria
- Any required field is missing from a subscription object
- `sender` is empty or null
- `count` is not a positive integer
