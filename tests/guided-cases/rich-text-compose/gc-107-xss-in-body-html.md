# gc-rtc-004: XSS in body_html Field

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: rich-text-compose
- **Tags**: xss, security, sanitization, body_html
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000
- Session token available via bootstrap endpoint

### Data
- At least one account configured (retrieve account_id via GET /api/accounts)

## Steps

1. Obtain session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap` with header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. List accounts to get a valid account_id
   - **Target**: `GET http://127.0.0.1:3000/api/accounts` with header `X-Session-Token: <token>`
   - **Expected**: 200 OK with JSON array containing at least one account; note the `id` field

3. Save a draft with XSS payload in body_html
   - **Target**: `POST http://127.0.0.1:3000/api/drafts` with header `X-Session-Token: <token>`
   - **Input**:
     ```json
     {
       "account_id": "<account_id from step 2>",
       "body_text": "Safe text",
       "body_html": "<script>alert('xss')</script><p>Safe text</p>"
     }
     ```
   - **Expected**: 200 OK with JSON body containing `draft_id` (server accepts the content; sanitization is enforced client-side via sandboxed iframe rendering)

4. Verify the draft was persisted
   - **Target**: `GET http://127.0.0.1:3000/api/drafts?account_id=<account_id>` with header `X-Session-Token: <token>`
   - **Expected**: 200 OK with JSON array containing the draft

5. Validate that XSS is mitigated at the rendering layer (informational)
   - **Target**: N/A (client-side concern)
   - **Expected**: When this draft is rendered in the UI, the EmailBody component uses a sandboxed iframe (`sandbox` attribute without `allow-scripts`) which prevents `<script>` execution. The regex sanitizer also strips dangerous patterns. This step documents the security boundary.

## Success Criteria
- [ ] POST /api/drafts returns 200 OK (server stores the content as-is)
- [ ] Response contains a non-empty `draft_id`
- [ ] The server does not crash or return an error on `<script>` content
- [ ] Security boundary is at the rendering layer (sandboxed iframe), not the storage layer

## Failure Criteria
- Server crashes or returns 500 on `<script>` tag in body_html
- Server silently drops the draft or corrupts the data
- If server-side sanitization is expected and does not strip `<script>`, this is a risk to document
