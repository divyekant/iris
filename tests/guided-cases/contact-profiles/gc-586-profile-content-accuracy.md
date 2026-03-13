# GC-586: Profile Content Accurately Reflects Email Exchange History

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: contact-profiles
- **Tags**: contact-profiles, content, accuracy, topics, AI
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)
- AI provider configured and healthy

### Data
- At least 10 messages with charlie@engineering.com predominantly about "Kubernetes deployment" and "CI/CD pipeline"

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Generate profile for charlie@engineering.com
   - **Target**: `POST http://localhost:3030/api/contacts/profiles/generate/charlie@engineering.com`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, profile returned

3. Verify profile topics reflect actual exchange content
   - **Target**: `GET http://localhost:3030/api/contacts/profiles/charlie@engineering.com`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, `topics` array includes terms related to Kubernetes or CI/CD; `summary` mentions technical collaboration

## Success Criteria
- [ ] `topics` array contains relevant technical terms from email history
- [ ] `summary` is coherent and describes the relationship accurately
- [ ] Profile does not contain hallucinated facts not present in email history
- [ ] `message_count` (if present) reflects actual number of exchanged messages

## Failure Criteria
- `topics` array empty despite clear topic patterns
- Summary contains fabricated details
- Profile for wrong contact returned
