# GC-552: HTML Tags Are Stripped Before Indexing HTML Attachment

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: attachment-search
- **Tags**: attachments, index, html, sanitization, FTS5
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- A message with an HTML attachment containing `<p>Project Alpha deliverable</p>` and surrounding markup tags

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Index the HTML attachment
   - **Target**: `POST http://localhost:3030/api/attachments/index/{message_id}`
   - **Input**: Header `X-Session-Token: {token}`, `message_id` of message with HTML attachment
   - **Expected**: 200 OK, `indexed_count` ≥ 1

3. Search for visible text from the HTML attachment
   - **Target**: `GET http://localhost:3030/api/attachments/search?q=Project+Alpha+deliverable&account_id={account_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, result found — snippet shows clean text without HTML tags

4. Search for a raw HTML tag that should NOT be indexed
   - **Target**: `GET http://localhost:3030/api/attachments/search?q=%3Cp%3E&account_id={account_id}`
   - **Input**: Header `X-Session-Token: {token}`, `q` = `<p>`
   - **Expected**: 200 OK, empty results (tags not indexed)

## Success Criteria
- [ ] Visible text is searchable after HTML stripping
- [ ] Snippet in results does not contain raw HTML tags
- [ ] Searching for literal `<p>` returns no results
- [ ] indexed_count reflects the HTML attachment was processed

## Failure Criteria
- HTML tags appear in search snippets
- Visible text not findable
- Indexing returns error on HTML input
