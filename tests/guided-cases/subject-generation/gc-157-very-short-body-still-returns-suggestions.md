# GC-157: Very Short Body (1 Word) Still Returns Suggestions

## Metadata
- **Type**: edge
- **Priority**: P2
- **Surface**: api
- **Flow**: subject-generation
- **Tags**: subject-generation, ai, edge, short-body, api
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000
- AI provider configured and healthy

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)

## Steps
1. Obtain a session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap`
   - **Input**: `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 with `token` field

2. Submit suggest-subject with a single-word body
   - **Target**: `POST http://127.0.0.1:3000/api/ai/suggest-subject`
   - **Input**: Header `X-Session-Token: <token>`, body `{"body": "Hello"}`
   - **Expected**: 200 with `{"suggestions": [...]}` containing at least 1 string (AI best-effort with minimal context)

3. Submit suggest-subject with a two-word body
   - **Target**: `POST http://127.0.0.1:3000/api/ai/suggest-subject`
   - **Input**: Header `X-Session-Token: <token>`, body `{"body": "Quick update"}`
   - **Expected**: 200 with suggestions array

4. Submit suggest-subject with a single punctuation character
   - **Target**: `POST http://127.0.0.1:3000/api/ai/suggest-subject`
   - **Input**: Header `X-Session-Token: <token>`, body `{"body": "."}`
   - **Expected**: 200 with suggestions array, or a 400 if the implementation treats punctuation-only as effectively empty (either outcome is acceptable, but no 500)

## Success Criteria
- [ ] Single-word body returns 200 with non-empty suggestions array
- [ ] Two-word body returns 200 with non-empty suggestions array
- [ ] Server does not crash for any of the minimal inputs
- [ ] Punctuation-only body returns either 200 or 400, not 500

## Failure Criteria
- Single-word body returns 400 (valid non-empty body was rejected)
- Any minimal body causes a 500
- Suggestions array is present but empty (silent failure)
