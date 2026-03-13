# GC-324: Body preview truncated at 500 chars

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: cc-suggestions
- **Tags**: cc-suggestions, edge-case, input-length, truncation, body-preview
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available
- AI provider configured and reachable

### Data
- At least one synced account with messages (source: prior sync)
- A contact `alice@example.com` with some co-occurrence history (source: seed or real inbox)

## Steps
1. POST to suggest-cc with a `body_preview` exactly 500 characters long
   - **Target**: `POST /api/ai/suggest-cc`
   - **Input**:
     ```json
     {
       "to": ["alice@example.com"],
       "cc": [],
       "subject": "Long body test",
       "body_preview": "<500-character string>"
     }
     ```
   - **Expected**: 200 OK — request accepted, suggestions returned normally

2. POST to suggest-cc with a `body_preview` of 1000 characters
   - **Target**: `POST /api/ai/suggest-cc`
   - **Input**:
     ```json
     {
       "to": ["alice@example.com"],
       "cc": [],
       "subject": "Long body test",
       "body_preview": "<1000-character string>"
     }
     ```
   - **Expected**: 200 OK — request accepted (backend truncates to 500 chars before sending to AI) OR 400 Bad Request if the endpoint enforces max length at the API boundary

3. POST to suggest-cc with an empty `body_preview` string
   - **Target**: `POST /api/ai/suggest-cc`
   - **Input**:
     ```json
     {
       "to": ["alice@example.com"],
       "cc": [],
       "subject": "No body",
       "body_preview": ""
     }
     ```
   - **Expected**: 200 OK — body preview is optional context; empty is accepted and suggestions rely on `to`/`subject` instead

## Success Criteria
- [ ] 500-character body preview accepted with 200
- [ ] Over-length body preview handled without 500 error (either truncated or rejected with 400)
- [ ] Empty body preview accepted with 200
- [ ] Suggestions are still returned for all accepted requests

## Failure Criteria
- 500 Internal Server Error for any body preview length
- Request with empty body preview rejected unnecessarily
- Over-length body preview causes AI prompt injection or context window errors propagated to client

## Notes
The `body_preview` field provides optional context for the AI. The field name implies truncation is expected. The endpoint should handle long values gracefully — either by truncating internally or rejecting with a clear 400. An empty string must be accepted since body content is not always available at composition time.
