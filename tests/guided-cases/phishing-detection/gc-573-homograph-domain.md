# GC-573: Homograph Domain Attack Detected (Unicode Lookalike Characters)

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: phishing-detection
- **Tags**: phishing, scan, homograph, unicode, domain, punycode
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- A message with a link to a homograph domain, e.g., "payраl.com" (using Cyrillic 'а' instead of Latin 'a'), which encodes to a punycode domain

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Scan the message with a homograph domain link
   - **Target**: `POST http://localhost:3030/api/security/phishing-scan/{message_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, `signals` includes a homograph or IDN spoofing signal; `risk_level` elevated

## Success Criteria
- [ ] Homograph domain generates a phishing signal
- [ ] Signal identifies the lookalike domain
- [ ] `risk_level` is at least `"medium"`

## Failure Criteria
- Homograph domain passes without detection
- Server crashes on non-ASCII domain names
