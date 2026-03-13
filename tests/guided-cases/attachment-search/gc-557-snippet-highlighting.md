# GC-557: Search Results Include FTS5 Snippet Highlighting

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: attachment-search
- **Tags**: attachments, search, snippet, highlight, FTS5
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- A message with a plain-text attachment containing the sentence: "The annual performance review process begins in November"
- Attachment has been indexed

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Index the attachment
   - **Target**: `POST http://localhost:3030/api/attachments/index/{message_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, `indexed_count: 1`

3. Search for a word in the attachment
   - **Target**: `GET http://localhost:3030/api/attachments/search?q=performance+review&account_id={account_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, result `snippet` field contains the search terms in context; terms may be wrapped in highlight markers (e.g., `**performance**` or `<b>performance</b>`)

## Success Criteria
- [ ] Search returns at least one result
- [ ] `snippet` field is present and non-empty
- [ ] Snippet contains the matched terms in surrounding context
- [ ] Snippet is limited to a reasonable length (not the entire document)

## Failure Criteria
- `snippet` is empty or missing
- Snippet contains the entire attachment text
- Terms not visible in snippet
