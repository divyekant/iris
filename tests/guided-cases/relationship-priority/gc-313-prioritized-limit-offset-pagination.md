# GC-313: Limit and Offset Pagination on Prioritized Messages

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: relationship-priority
- **Tags**: relationship-priority, pagination, limit, offset, prioritized-messages
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- At least 10 messages in the inbox for the test account (source: prior sync or seed)
- Relationship scores computed (source: POST /api/ai/relationship-priority)

## Steps

1. Obtain a session token
   - **Target**: `GET http://localhost:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Fetch first page with limit=5
   - **Target**: `GET http://localhost:3000/api/messages/prioritized?account_id={account_id}&folder=INBOX&limit=5&offset=0`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK; `messages` array length <= 5; `total` reflects full result set size

3. Fetch second page with offset=5
   - **Target**: `GET http://localhost:3000/api/messages/prioritized?account_id={account_id}&folder=INBOX&limit=5&offset=5`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK; `messages` array length <= 5; message IDs do not overlap with page 1

4. Verify non-overlapping pages
   - **Target**: Message IDs from steps 2 and 3
   - **Input**: Collect all message `id` values from both pages
   - **Expected**: No duplicate IDs across the two pages; combined count matches min(total, 10)

5. Verify total is consistent across pages
   - **Target**: `total` field from steps 2 and 3
   - **Input**: Compare `total` values
   - **Expected**: `total` is identical in both responses (it reflects the full result set, not just the page)

## Success Criteria
- [ ] Step 2 returns at most 5 messages
- [ ] Step 3 returns at most 5 messages
- [ ] No message ID appears in both pages
- [ ] `total` is the same in both page responses
- [ ] `total` >= messages returned in step 2 (it is the full count, not page count)

## Failure Criteria
- Either page returns more messages than the requested limit
- Duplicate message IDs across pages
- `total` differs between page requests
- Server returns non-200 for either request

## Notes
Pagination correctness is critical for large inboxes. The `total` field must always be the full unsliced count so clients can calculate page count. Default limit is 50 per the API spec; this test explicitly overrides it to 5 to verify the param is respected.
