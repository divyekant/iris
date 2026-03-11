# GC-155: AI Disabled Returns Appropriate Error

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: subject-generation
- **Tags**: subject-generation, ai-disabled, negative, configuration, api
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000
- AI provider is **not** configured or all providers are unreachable / disabled

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)
- AI settings either: (a) Ollama URL pointing to a non-running host, or (b) no API keys set and Ollama unavailable

## Steps
1. Obtain a session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap`
   - **Input**: `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 with `token` field

2. Confirm AI is unavailable via health endpoint
   - **Target**: `GET http://127.0.0.1:3000/api/health`
   - **Input**: Header `X-Session-Token: <token>`
   - **Expected**: 200 with health response where `ai` or `ollama` status is not "ok"

3. Submit suggest-subject request while AI is disabled/unavailable
   - **Target**: `POST http://127.0.0.1:3000/api/ai/suggest-subject`
   - **Input**: Header `X-Session-Token: <token>`, body `{"body": "Let me know if Thursday works for a call."}`
   - **Expected**: Non-200 error response (e.g., 503 Service Unavailable or 500) with a JSON error body describing the AI unavailability — not a silent empty suggestions array

## Success Criteria
- [ ] Response is not 200 when AI is unavailable
- [ ] Response body contains a JSON error message (not an empty suggestions array)
- [ ] Error message is informative (e.g., references AI provider or availability)
- [ ] Server does not panic or return an unstructured HTML error

## Failure Criteria
- Response is 200 with an empty suggestions array (silent failure)
- Server returns a non-JSON body (HTML error page)
- Server crashes (process-level failure)
