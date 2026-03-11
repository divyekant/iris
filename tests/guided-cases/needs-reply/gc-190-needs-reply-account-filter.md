# GC-190: Needs-Reply Filter by Account ID

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: needs-reply
- **Tags**: needs-reply, api, filter, account
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000

### Data
- At least one email account synced with messages flagged as needs-reply
- Session token obtained via GET /api/auth/bootstrap
- A valid `account_id` from GET /api/accounts

## Steps

1. Fetch needs-reply messages filtered by account
   - **Target**: `GET /api/messages/needs-reply?account_id={account_id}`
   - **Input**: Valid `X-Session-Token` header; valid `account_id` query param
   - **Expected**: 200 OK with JSON body; all returned messages belong to the specified account

2. Verify all messages belong to the requested account
   - **Target**: Each object in the `messages` array
   - **Input**: n/a
   - **Expected**: Every message has `account_id` matching the query parameter

## Success Criteria
- [ ] Response status is 200
- [ ] Every message in `messages` has `account_id` equal to the requested value
- [ ] `total` reflects the filtered count

## Failure Criteria
- Response status is not 200
- Messages from other accounts appear in the result
- `account_id` field is absent from message objects
