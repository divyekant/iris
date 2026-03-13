# GC-361: SSN Detected (123-45-6789)

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: dlp
- **Tags**: ssn, detection, masking, risk-high, pii
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`

### Data
- Email body containing SSN `123-45-6789` in XXX-XX-XXXX format (source: manual input)

## Steps

1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Submit an email containing a Social Security Number
   - **Target**: `POST http://localhost:3030/api/compose/scan-dlp`
   - **Input**: Header `X-Session-Token: {token}`, Body:
     ```json
     {
       "subject": "Employee onboarding form",
       "body": "Please use SSN 123-45-6789 for the background check form.",
       "to": ["hr@example.com"]
     }
     ```
   - **Expected**: 200 OK with JSON response

3. Validate SSN finding is reported
   - **Target**: Response from step 2
   - **Input**: Inspect `findings` array
   - **Expected**: `findings` contains one entry with `type: "ssn"`, `location: "body"`, and `match` masked (the raw SSN is not present in the response)

4. Validate risk level
   - **Target**: Response from step 2
   - **Input**: Inspect `risk_level` and `allow_send`
   - **Expected**: `risk_level` is `"high"`, `allow_send` is `false`

## Success Criteria
- [ ] Response status is 200
- [ ] `findings` contains exactly one entry with `type: "ssn"`
- [ ] Finding `location` is `"body"`
- [ ] Finding `match` does not contain the raw SSN digits unmasked
- [ ] `risk_level` is `"high"`
- [ ] `allow_send` is `false`

## Failure Criteria
- SSN is not detected
- `match` field exposes the full unmasked SSN
- `risk_level` is not `"high"`
- `allow_send` is `true` when SSN finding is present

## Notes
`123-45-6789` passes the XXX-XX-XXXX format check. The SSN filter also rejects all-same-digit patterns (e.g., `111-11-1111`) and sequential patterns — this case uses a non-trivial value that should pass through to detection. SSN is classified as high risk, setting `allow_send: false`.
