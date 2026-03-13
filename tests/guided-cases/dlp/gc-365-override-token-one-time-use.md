# GC-365: Override Token Generated and Is One-Time Use

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: dlp
- **Tags**: dlp-override, one-time-token, token-invalidation, send-flow
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`

### Data
- A DLP scan that returns at least one finding (to justify requesting an override token) — use body `"My SSN is 123-45-6789"` to trigger a finding

## Steps

1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Request an override token with `findings_acknowledged: true`
   - **Target**: `POST http://localhost:3030/api/compose/dlp-override`
   - **Input**: Header `X-Session-Token: {token}`, Body: `{"findings_acknowledged": true}`
   - **Expected**: 200 OK with JSON body `{"token": "<uuid-string>"}`

3. Validate the token is a valid UUID
   - **Target**: Response from step 2
   - **Input**: Inspect `token` field
   - **Expected**: `token` is a non-empty string in UUID format (e.g., `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`)

4. Attempt to use the override token a second time
   - **Target**: `POST http://localhost:3030/api/compose/dlp-override`
   - **Input**: Header `X-Session-Token: {token}`, Body: `{"findings_acknowledged": true}`
   - **Expected**: A new, different UUID token is returned (each call generates a fresh token)

5. Simulate consuming the first override token (use it to send)
   - **Target**: `POST http://localhost:3030/api/messages/send` (or the send endpoint that accepts a DLP override token)
   - **Input**: Header `X-Session-Token: {token}`, include override token from step 2 in the send request
   - **Expected**: The first token is accepted once; a second attempt to use the same token is rejected (4xx response)

## Success Criteria
- [ ] Response status is 200 on the override request
- [ ] `token` field is a non-empty UUID string
- [ ] Each call to `/api/compose/dlp-override` returns a distinct token
- [ ] An override token cannot be reused after it has been consumed — second use returns a 4xx error

## Failure Criteria
- Override endpoint does not return a token
- The same token can be used more than once (one-time-use constraint violated)
- Token format is not a UUID

## Notes
`validate_override_token` removes the token on first use, making it single-use. This prevents a user from bypassing DLP permanently by reusing one override decision. Step 5 may require adapting to the exact send endpoint signature that accepts a DLP token; if the send endpoint is not yet implemented, verify one-time-use by calling a hypothetical `/api/compose/validate-dlp-token` or checking that the server-side token store no longer contains the token after first use.
