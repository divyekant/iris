# GC-367: Luhn Validation — Random 16-Digit Number NOT Flagged if Invalid

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: dlp
- **Tags**: luhn, false-positive, credit-card, edge-case, validation
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`

### Data
- A 16-digit number starting with `4` (Visa prefix) that fails Luhn check: `4111111111111112` — last digit changed to make it invalid (source: manual construction)

## Steps

1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Submit an email containing a 16-digit Visa-prefix number that fails Luhn validation
   - **Target**: `POST http://localhost:3030/api/compose/scan-dlp`
   - **Input**: Header `X-Session-Token: {token}`, Body:
     ```json
     {
       "subject": "Order reference",
       "body": "Your order reference number is 4111111111111112. Please keep this for your records.",
       "to": ["customer@example.com"]
     }
     ```
   - **Expected**: 200 OK with JSON response

3. Validate that the number is NOT flagged as a credit card
   - **Target**: Response from step 2
   - **Input**: Inspect `findings` array
   - **Expected**: `findings` is empty — `4111111111111112` does not pass Luhn check and must not be reported as a credit card

4. Validate risk level reflects no findings
   - **Target**: Response from step 2
   - **Input**: Inspect `risk_level` and `allow_send`
   - **Expected**: `risk_level` is `"none"`, `allow_send` is `true`

5. Cross-check: confirm the valid variant IS flagged
   - **Target**: `POST http://localhost:3030/api/compose/scan-dlp`
   - **Input**: Same request but body contains `4111111111111111` (valid Luhn)
   - **Expected**: `findings` is non-empty, `type: "credit_card"` reported — confirming the difference is purely Luhn validity

## Success Criteria
- [ ] `4111111111111112` (Luhn-invalid) produces `findings: []`
- [ ] `risk_level` is `"none"` and `allow_send` is `true` for the invalid number
- [ ] `4111111111111111` (Luhn-valid) produces a `credit_card` finding in the cross-check step

## Failure Criteria
- `4111111111111112` is flagged as a credit card (false positive — Luhn check not applied)
- Both the valid and invalid variants produce the same result
- Cross-check fails: `4111111111111111` is also not detected

## Notes
Luhn validation is a critical false-positive guard. Order numbers, product IDs, and other numeric sequences that happen to be 16 digits must not be flagged unless they pass Luhn and match a known card prefix. The Luhn algorithm: sum of digits from right, double every second digit from right, sum all, must be divisible by 10. `4111111111111111` passes; `4111111111111112` fails because the checksum becomes 51, not a multiple of 10.
