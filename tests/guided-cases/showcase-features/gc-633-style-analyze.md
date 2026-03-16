# GC-633: Writing Style — POST /api/style/analyze Extracts Writing Traits from Sent Emails

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: showcase-features
- **Tags**: writing-style, analysis, sent-emails, traits, tone
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)
- AI provider available (Ollama, Anthropic, or OpenAI)

### Data
- Account `{account_id}` has at least 5 sent emails in the database (folder = 'Sent')
- Sent emails have varied greetings and sign-offs (e.g., "Hi", "Hello", "Best", "Thanks")

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Trigger style analysis for the account
   - **Target**: `POST http://localhost:3030/api/style/analyze/{account_id}`
   - **Input**: Header `X-Session-Token: {token}`, path param `account_id`
   - **Expected**: 200 OK, response contains `traits` object with extracted style properties

3. Verify traits structure
   - **Target**: `traits` object from step 2
   - **Input**: inspect top-level fields
   - **Expected**: `traits` contains at minimum `greeting` (string), `signoff` (string), `tone` (e.g., `formal`, `casual`, `neutral`), `messages_analyzed` (integer ≥ 5)

4. Verify persistence
   - **Target**: `GET http://localhost:3030/api/style/{account_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, returns the same traits just computed

## Success Criteria
- [ ] POST /api/style/analyze/{account_id} returns 200 OK
- [ ] `traits.greeting` is a non-empty string
- [ ] `traits.signoff` is a non-empty string
- [ ] `traits.tone` is one of `formal`, `casual`, `neutral`
- [ ] `traits.messages_analyzed` ≥ 5
- [ ] Subsequent GET /api/style/{account_id} returns identical traits

## Failure Criteria
- 404 if account not found
- 422 if fewer than a minimum number of sent emails exist (document minimum)
- `traits` missing `greeting`, `signoff`, or `tone`
- Analysis result not persisted (GET returns 404 after successful POST)
