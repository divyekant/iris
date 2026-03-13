# GC-558: Large Attachment Content Is Indexed Without Timeout or Error

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: attachment-search
- **Tags**: attachments, index, large-content, performance
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- A message with a large plain-text attachment (≥ 500KB of text content), containing the phrase "strategic alignment framework" near the end

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Index the large attachment
   - **Target**: `POST http://localhost:3030/api/attachments/index/{message_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK within a reasonable time (< 30 seconds), `indexed_count: 1`; or a 202 Accepted if processing is asynchronous

3. Search for a phrase near the end of the large attachment
   - **Target**: `GET http://localhost:3030/api/attachments/search?q=strategic+alignment+framework&account_id={account_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, result found with accurate snippet

## Success Criteria
- [ ] Index does not timeout or return 5xx for large content
- [ ] Index either returns 200 (sync) or 202 (async) — not an error
- [ ] Large attachment content is searchable
- [ ] Snippet is contextually accurate (not truncated mid-word)

## Failure Criteria
- Server times out or returns 5xx during indexing
- Content not searchable after indexing
- Memory or resource exhaustion errors
