# gc-rtc-002: Send Email with HTML Body

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: rich-text-compose
- **Tags**: send, html, body_html, email
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000
- Session token available via bootstrap endpoint

### Data
- At least one active account configured (retrieve via GET /api/accounts)
- Note: actual SMTP delivery may fail if no real credentials are configured; the test validates that the API accepts the request and queues the send

## Steps

1. Obtain session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap` with header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. List accounts to get a valid account_id
   - **Target**: `GET http://127.0.0.1:3000/api/accounts` with header `X-Session-Token: <token>`
   - **Expected**: 200 OK with JSON array containing at least one account with `is_active: true`; note the `id` field

3. Send an email with both body_text and body_html
   - **Target**: `POST http://127.0.0.1:3000/api/send` with header `X-Session-Token: <token>`
   - **Input**:
     ```json
     {
       "account_id": "<account_id from step 2>",
       "to": ["test@example.com"],
       "subject": "HTML Test",
       "body_text": "Hello",
       "body_html": "<p><em>Hello</em></p>"
     }
     ```
   - **Expected**: 200 OK with JSON body containing `id` (non-empty string), `send_at` (integer), `can_undo` (boolean), and `scheduled` (boolean)

4. Verify the send was queued by checking that the id is non-empty
   - **Target**: Inspect response from step 3
   - **Expected**: `id` is a UUID-format string, `can_undo` is `true` (undo-send window), `scheduled` is `false`

## Success Criteria
- [ ] POST /api/send returns 200 OK
- [ ] Response contains `id`, `send_at`, `can_undo`, and `scheduled` fields
- [ ] `id` is a non-empty string
- [ ] `can_undo` is `true` (default undo delay applies)
- [ ] `scheduled` is `false`

## Failure Criteria
- POST /api/send returns a non-200 status code
- Response is missing required fields (`id`, `send_at`, `can_undo`, `scheduled`)
- Account not found (404) indicates no active account is configured
