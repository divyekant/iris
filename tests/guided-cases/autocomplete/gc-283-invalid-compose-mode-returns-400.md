# GC-283: Invalid compose_mode Returns 400

## Metadata
- **Type**: negative
- **Priority**: P0
- **Surface**: api
- **Flow**: autocomplete
- **Tags**: autocomplete, ai, compose, validation, invalid-input, 400
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- No specific data required — validation should fire before any AI or DB call

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Send autocomplete request with an unrecognized compose_mode
   - **Target**: `POST http://localhost:3030/api/ai/autocomplete`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3030/api/ai/autocomplete \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{
         "partial_text": "Please find attached",
         "cursor_position": 20,
         "compose_mode": "fax"
       }'
     ```
   - **Expected**: 400 Bad Request with error body describing the invalid `compose_mode`

3. Send autocomplete request with a numeric compose_mode
   - **Target**: `POST http://localhost:3030/api/ai/autocomplete`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3030/api/ai/autocomplete \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{
         "partial_text": "Please find attached",
         "cursor_position": 20,
         "compose_mode": 42
       }'
     ```
   - **Expected**: 400 Bad Request — type mismatch; `compose_mode` must be a string enum

4. Send autocomplete request with compose_mode omitted entirely
   - **Target**: `POST http://localhost:3030/api/ai/autocomplete`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3030/api/ai/autocomplete \
       -H "X-Session-Token: $TOKEN" \
       -H "Content-Type: application/json" \
       -d '{
         "partial_text": "Please find attached",
         "cursor_position": 20
       }'
     ```
   - **Expected**: 400 Bad Request — `compose_mode` is required

## Success Criteria
- [ ] Invalid string enum value returns 400
- [ ] Wrong type (numeric) returns 400
- [ ] Missing required field returns 400
- [ ] Error response body contains a message identifying the offending field
- [ ] No 500 Internal Server Error

## Failure Criteria
- 200 OK returned for any invalid `compose_mode` value
- 500 Internal Server Error instead of 400
- No error body — response is silent or empty on rejection
- AI is invoked despite invalid input (wasted compute)

## Notes
Valid values for `compose_mode` are: `"new"`, `"reply"`, `"reply_all"`, `"forward"`. Any other value must be rejected with 400 at the deserialization/validation layer before reaching AI logic.
