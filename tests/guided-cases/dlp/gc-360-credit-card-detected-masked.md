# GC-360: Credit Card Detected and Masked (Visa 4111111111111111)

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: dlp
- **Tags**: credit-card, detection, masking, luhn, visa, risk-high
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`

### Data
- Email body containing a Visa test card number `4111111111111111` (source: manual input)

## Steps

1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Submit an email containing a Visa credit card number
   - **Target**: `POST http://localhost:3030/api/compose/scan-dlp`
   - **Input**: Header `X-Session-Token: {token}`, Body:
     ```json
     {
       "subject": "Payment details",
       "body": "Please charge card number 4111111111111111 for the invoice.",
       "to": ["billing@example.com"]
     }
     ```
   - **Expected**: 200 OK with JSON response

3. Validate finding is reported
   - **Target**: Response from step 2
   - **Input**: Inspect `findings` array
   - **Expected**: `findings` contains exactly one entry with `type: "credit_card"`, `location: "body"`, and `match` masked as `"4111****1111"`

4. Validate risk level and send flag
   - **Target**: Response from step 2
   - **Input**: Inspect `risk_level` and `allow_send`
   - **Expected**: `risk_level` is `"high"`, `allow_send` is `false`

## Success Criteria
- [ ] Response status is 200
- [ ] `findings` contains exactly one entry
- [ ] Finding `type` is `"credit_card"`
- [ ] Finding `location` is `"body"`
- [ ] Finding `match` is `"4111****1111"` (first 4 + `****` + last 4)
- [ ] `risk_level` is `"high"`
- [ ] `allow_send` is `false`

## Failure Criteria
- Credit card number is not detected
- `match` field contains the unmasked card number
- `risk_level` is not `"high"`
- `allow_send` is `true` when a credit card finding is present

## Notes
`4111111111111111` is the canonical Visa test card: passes Luhn check, starts with `4` (Visa prefix). The masked value should be `4111****1111` — first 4 + stars + last 4.
