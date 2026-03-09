# gc-rtc-003: Draft Without body_html (Plain Text Fallback)

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: rich-text-compose
- **Tags**: draft, plain-text, fallback, optional
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

3. Save a draft with only body_text (no body_html field)
   - **Target**: `POST http://127.0.0.1:3000/api/drafts` with header `X-Session-Token: <token>`
   - **Input**:
     ```json
     {
       "account_id": "<account_id from step 2>",
       "body_text": "This is a plain text draft without any HTML body."
     }
     ```
   - **Expected**: 200 OK with JSON body containing `draft_id` (non-empty string)

4. Verify the draft was persisted
   - **Target**: `GET http://127.0.0.1:3000/api/drafts?account_id=<account_id>` with header `X-Session-Token: <token>`
   - **Expected**: 200 OK with JSON array containing a draft whose ID matches the `draft_id` from step 3

## Success Criteria
- [ ] POST /api/drafts returns 200 OK even without `body_html` field
- [ ] Response contains a non-empty `draft_id` string
- [ ] The server does not reject the request for missing `body_html`
- [ ] Draft appears in the listing

## Failure Criteria
- POST /api/drafts returns 400 or 422 due to missing `body_html`
- Server crashes or returns 500
- Draft is not persisted
