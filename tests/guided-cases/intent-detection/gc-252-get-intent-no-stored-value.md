# GC-252: Get Intent for Message with No Stored Intent

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: intent-detection
- **Tags**: intent, ai, classification, null, unprocessed, edge
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Session token obtained via `GET /api/auth/bootstrap`

### Data
- A message ID for a message that exists in the database but whose `intent` column value is NULL — i.e., it has never been through detect-intent (source: a newly synced message before AI pipeline runs, or a message whose intent was cleared)
- The message must exist (so the expected behavior is different from GC-251, which uses a non-existent ID)

## Steps

1. Obtain a session token
   - **Target**: `GET http://localhost:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with `token` field; store as `$TOKEN`

2. Identify a message with no stored intent (or force one via SQLite)
   - **Option A (preferred)**: Sync a new message while AI pipeline is paused; use that message's ID
   - **Option B (direct)**: Use SQLite CLI to clear intent for a known message:
     ```
     sqlite3 iris.db "UPDATE messages SET intent = NULL WHERE id = '$MESSAGE_ID';"
     ```
   - **Expected**: Message exists in DB with `intent IS NULL`

3. Call the GET stored-intent endpoint for this message
   - **Target**: `GET http://localhost:3000/api/messages/$MESSAGE_ID/intent`
   - **Input**:
     ```
     curl -s http://localhost:3000/api/messages/$MESSAGE_ID/intent \
       -H "x-session-token: $TOKEN"
     ```
   - **Expected**: Either:
     - 200 OK with `{"intent": null}` (key present, value null), **or**
     - 404 Not Found with an error body indicating intent has not been computed
   - **Not acceptable**: 200 OK with a fabricated or empty-string intent value

4. Verify the message still appears in the message list (it was not deleted)
   - **Target**: `GET http://localhost:3000/api/messages/$MESSAGE_ID`
   - **Input**: Header `x-session-token: $TOKEN`
   - **Expected**: 200 OK with the message detail; `intent` field is null or absent

## Success Criteria
- [ ] GET intent endpoint handles a null-intent message without crashing (no 500)
- [ ] Response is either 200 with `intent: null` or 404 with an error body — either is acceptable; document which behavior the implementation uses
- [ ] Response body does not contain a fabricated intent value
- [ ] The message itself is still retrievable via GET /api/messages/{id}

## Failure Criteria
- Endpoint returns 500 for a valid message ID with a null intent
- Endpoint returns a non-null, fabricated `intent` string
- Endpoint returns 200 with an empty-string `intent` (empty string is not a valid enum value)
- The message becomes unretrievable after this call

## Cleanup
- If SQLite was used to clear intent in step 2, optionally restore: `sqlite3 iris.db "UPDATE messages SET intent = NULL WHERE id = '$MESSAGE_ID';"` (already null — no action needed)
