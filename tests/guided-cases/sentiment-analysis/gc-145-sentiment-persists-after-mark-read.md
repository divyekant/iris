# GC-145: Sentiment Persists After Mark Read

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: sentiment-analysis
- **Tags**: sentiment-analysis, api, mark-read, persistence, edge
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions

### Environment
- App running at http://127.0.0.1:3000
- At least one email account synced

### Data
- A message ID with a known non-null `ai_sentiment` value (record the value before the test)
- The message is currently unread (`is_read = false`)
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Fetch the message list and record the baseline `ai_sentiment` for the target message
   - **Target**: `GET /api/messages?account_id={account_id}&folder=INBOX`
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: 200 OK; note `ai_sentiment` value (e.g., `"positive"`) for the chosen message ID

2. Mark the message as read
   - **Target**: `PATCH /api/messages/{id}/read`
   - **Input**: Valid `X-Session-Token` header; body `{"is_read": true}`
   - **Expected**: 200 OK or 204 No Content

3. Re-fetch the message detail to verify sentiment is unchanged
   - **Target**: `GET /api/messages/{id}`
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: 200 OK; `ai_sentiment` equals the baseline value recorded in step 1; `is_read` is now `true`

4. Re-fetch the message list to verify sentiment is unchanged in list context
   - **Target**: `GET /api/messages?account_id={account_id}&folder=INBOX`
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: 200 OK; the same message still has the same `ai_sentiment` value

## Success Criteria
- [ ] `PATCH /api/messages/{id}/read` succeeds (200 or 204)
- [ ] `ai_sentiment` value is identical before and after the mark-read operation
- [ ] `is_read` is `true` in the post-operation detail response

## Failure Criteria
- `ai_sentiment` changes or becomes null after marking the message as read
- The mark-read operation returns a non-2xx status
- Post-operation detail response is missing the `ai_sentiment` field
