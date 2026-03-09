# gc-rtc-001: Save Draft with HTML Body

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: rich-text-compose
- **Tags**: draft, html, body_html, save
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

3. Save a draft with both body_text and body_html
   - **Target**: `POST http://127.0.0.1:3000/api/drafts` with header `X-Session-Token: <token>`
   - **Input**:
     ```json
     {
       "account_id": "<account_id from step 2>",
       "body_text": "Hello world",
       "body_html": "<p>Hello <strong>world</strong></p>"
     }
     ```
   - **Expected**: 200 OK with JSON body containing `draft_id` (non-empty string)

4. Verify the draft was persisted by listing drafts
   - **Target**: `GET http://127.0.0.1:3000/api/drafts?account_id=<account_id>` with header `X-Session-Token: <token>`
   - **Expected**: 200 OK with JSON array containing a draft whose ID matches the `draft_id` from step 3

## Success Criteria
- [ ] POST /api/drafts returns 200 OK
- [ ] Response contains a non-empty `draft_id` string
- [ ] Draft appears in the list of drafts for the account
- [ ] No error or 4xx/5xx response at any step

## Failure Criteria
- POST /api/drafts returns a non-200 status code
- Response body is missing the `draft_id` field
- Draft does not appear in the listing after creation
