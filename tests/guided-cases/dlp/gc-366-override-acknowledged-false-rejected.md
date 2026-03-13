# GC-366: Override with findings_acknowledged=false Rejected

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: dlp
- **Tags**: dlp-override, validation, negative, findings-acknowledged
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`

### Data
- No specific email data required — this test exercises request validation only

## Steps

1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Request an override token with `findings_acknowledged: false`
   - **Target**: `POST http://localhost:3030/api/compose/dlp-override`
   - **Input**: Header `X-Session-Token: {token}`, Body: `{"findings_acknowledged": false}`
   - **Expected**: 4xx error response (400 Bad Request or 422 Unprocessable Entity)

3. Verify no token is returned
   - **Target**: Response from step 2
   - **Input**: Inspect response body
   - **Expected**: Response body does NOT contain a `token` field; error message indicates acknowledgement is required

4. Request an override token with the field omitted entirely
   - **Target**: `POST http://localhost:3030/api/compose/dlp-override`
   - **Input**: Header `X-Session-Token: {token}`, Body: `{}`
   - **Expected**: 4xx error response; no token issued

## Success Criteria
- [ ] Request with `findings_acknowledged: false` returns a 4xx status
- [ ] Response body does not contain a `token` field
- [ ] Request with `findings_acknowledged` omitted returns a 4xx status
- [ ] Error response contains a meaningful message (not a generic 500)

## Failure Criteria
- A token is returned when `findings_acknowledged` is `false`
- A token is returned when the field is missing
- Server returns 500 instead of a validation error
- The endpoint issues a token without explicit user acknowledgement

## Notes
`findings_acknowledged` must be explicitly `true` to receive an override token. This ensures users cannot bypass DLP by accident or via automated requests that omit the acknowledgement field. The server must validate the boolean value, not just the field's presence.
