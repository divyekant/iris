# GC-314: Limit Capped at 500

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: relationship-priority
- **Tags**: relationship-priority, limit-cap, edge-case, input-validation, prioritized-messages
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- At least one message in the inbox (source: prior sync)
- Relationship scores computed (source: POST /api/ai/relationship-priority)

## Steps

1. Obtain a session token
   - **Target**: `GET http://localhost:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Request prioritized messages with an extreme limit value
   - **Target**: `GET http://localhost:3000/api/messages/prioritized?account_id={account_id}&folder=INBOX&limit=9999`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK; `messages` array length <= 500; no server error

3. Verify the response does not exceed the cap
   - **Target**: Response JSON from step 2
   - **Input**: Count elements in `messages` array
   - **Expected**: `messages.length` <= 500 regardless of how many messages exist in the inbox

4. Repeat with limit=1000000
   - **Target**: `GET http://localhost:3000/api/messages/prioritized?account_id={account_id}&folder=INBOX&limit=1000000`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK; `messages` array length <= 500

## Success Criteria
- [ ] Both requests return 200 OK
- [ ] Neither response contains more than 500 messages
- [ ] No 400 or 500 error returned for oversized limit values
- [ ] Server does not appear to attempt an unbounded DB scan (responds in reasonable time)

## Failure Criteria
- Server returns 500 or crashes on large limit value
- Response contains more than 500 message objects
- Request times out due to unbounded query execution

## Notes
The API spec states max limit is 500. This prevents runaway queries on large inboxes. The server should silently clamp the value rather than returning a 400 error — accepting the request but honouring the cap is the expected UX. Verify response time is comparable to a limit=500 request.
