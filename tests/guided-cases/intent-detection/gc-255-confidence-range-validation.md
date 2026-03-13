# GC-255: Confidence Value Is Between 0.0 and 1.0

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: intent-detection
- **Tags**: intent, ai, classification, confidence, range, validation
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Session token obtained via `GET /api/auth/bootstrap`
- AI provider configured and enabled

### Data
- At least three message IDs representing different content clarity levels (source: synced inbox):
  - A message with clear, unambiguous intent (expected: high confidence)
  - A message with ambiguous or multi-intent content (expected: lower confidence)
  - A message with obvious category content, e.g., a newsletter (expected: high confidence)

## Steps

1. Obtain a session token
   - **Target**: `GET http://localhost:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with `token` field; store as `$TOKEN`

2. Detect intent for a clear-intent message
   - **Target**: `POST http://localhost:3000/api/ai/detect-intent`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3000/api/ai/detect-intent \
       -H "x-session-token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"message_id": "$CLEAR_MESSAGE_ID"}'
     ```
   - **Expected**: 200 OK with `{"intent": "<value>", "confidence": <float>}`; `confidence` is between 0.0 and 1.0 inclusive

3. Detect intent for an ambiguous message
   - **Target**: `POST http://localhost:3000/api/ai/detect-intent`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3000/api/ai/detect-intent \
       -H "x-session-token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"message_id": "$AMBIGUOUS_MESSAGE_ID"}'
     ```
   - **Expected**: 200 OK; `confidence` is between 0.0 and 1.0 inclusive (likely lower than step 2)

4. Detect intent for a newsletter message
   - **Target**: `POST http://localhost:3000/api/ai/detect-intent`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3000/api/ai/detect-intent \
       -H "x-session-token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{"message_id": "$NEWSLETTER_MESSAGE_ID"}'
     ```
   - **Expected**: 200 OK; `confidence` is between 0.0 and 1.0 inclusive

5. Verify all three confidence values are within the valid range [0.0, 1.0]
   - **Target**: All three response bodies
   - **Input**: n/a
   - **Expected**: For each response: `0.0 <= confidence <= 1.0`; `confidence` is a JSON number (not a string, not null, not absent)

## Success Criteria
- [ ] All three detect-intent calls return 200
- [ ] All three `confidence` values are JSON numbers
- [ ] All three `confidence` values satisfy: `0.0 <= confidence <= 1.0`
- [ ] `confidence` is never null, absent, or returned as a string

## Failure Criteria
- `confidence` is greater than 1.0 (indicates the API is returning a raw AI score rather than a normalized probability)
- `confidence` is less than 0.0 (negative value is not a valid probability)
- `confidence` is returned as a string (e.g., `"0.92"` instead of `0.92`)
- `confidence` is null or absent from the response body
- Any response returns a non-200 status
