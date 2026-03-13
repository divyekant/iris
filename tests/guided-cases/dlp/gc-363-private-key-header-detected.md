# GC-363: Private Key Header Detected

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: dlp
- **Tags**: private-key, detection, masking, risk-high, pem
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`

### Data
- Email body containing a PEM private key header `-----BEGIN RSA PRIVATE KEY-----` (source: manual input)

## Steps

1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Submit an email containing a private key PEM header in the body
   - **Target**: `POST http://localhost:3030/api/compose/scan-dlp`
   - **Input**: Header `X-Session-Token: {token}`, Body:
     ```json
     {
       "subject": "Server credentials",
       "body": "Attached below is the private key for the deployment server:\n\n-----BEGIN RSA PRIVATE KEY-----\nMIIEowIBAAKCAQEA0Z3VS5JJcds3xHn/ygWep4\n-----END RSA PRIVATE KEY-----",
       "to": ["ops@example.com"]
     }
     ```
   - **Expected**: 200 OK with JSON response

3. Validate private key finding is reported
   - **Target**: Response from step 2
   - **Input**: Inspect `findings` array
   - **Expected**: `findings` contains at least one entry with `type: "private_key"`, `location: "body"`, and a masked `match` value

4. Validate risk level
   - **Target**: Response from step 2
   - **Input**: Inspect `risk_level` and `allow_send`
   - **Expected**: `risk_level` is `"high"`, `allow_send` is `false`

## Success Criteria
- [ ] Response status is 200
- [ ] `findings` contains at least one entry with `type: "private_key"`
- [ ] Finding `location` is `"body"`
- [ ] `risk_level` is `"high"`
- [ ] `allow_send` is `false`

## Failure Criteria
- Private key header is not detected
- `risk_level` is not `"high"` when a private key finding is present
- `allow_send` is `true` when a private key is detected

## Notes
The detection pattern is `-----BEGIN (RSA |EC )?PRIVATE KEY-----`. Both generic `PRIVATE KEY`, `RSA PRIVATE KEY`, and `EC PRIVATE KEY` headers should match. Private key presence is classified as high risk regardless of whether it's a single finding. Test verifies both body-location detection and correct risk classification.
