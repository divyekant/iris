# GC-146: Invalid Sentiment Value in DB Handled Gracefully

## Metadata
- **Type**: negative
- **Priority**: P2
- **Surface**: api
- **Flow**: sentiment-analysis
- **Tags**: sentiment-analysis, api, invalid-value, robustness, negative
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions

### Environment
- App running at http://127.0.0.1:3000
- Direct SQLite access available (e.g., via `sqlite3 iris.db`)

### Data
- A known message ID in the INBOX folder
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Directly write an out-of-enum value into `ai_sentiment` in the SQLite database
   - **Target**: SQLite CLI or equivalent: `UPDATE messages SET ai_sentiment = 'ecstatic' WHERE id = '{message_id}';`
   - **Input**: The chosen message ID; the invalid value `"ecstatic"`
   - **Expected**: SQLite accepts the write (the column has no CHECK constraint; this is expected behavior at the DB layer)

2. Fetch the message list via the API
   - **Target**: `GET /api/messages?account_id={account_id}&folder=INBOX`
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: 200 OK; the affected message is present; `ai_sentiment` value is `"ecstatic"` (the API passes the raw DB value through without crashing)

3. Fetch the message detail
   - **Target**: `GET /api/messages/{id}`
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: 200 OK; `ai_sentiment` is `"ecstatic"`; no 500 error

4. Open the inbox in the browser and locate the message row
   - **Target**: http://127.0.0.1:3000/#/
   - **Input**: n/a
   - **Expected**: No sentiment pill is rendered for this row, because `"ecstatic"` is not a key in the `sentimentConfig` map (`sentimentConfig["ecstatic"]` is undefined, so the `{#if}` guard suppresses rendering)

## Success Criteria
- [ ] API returns 200 for both list and detail endpoints when `ai_sentiment` contains an unrecognized value
- [ ] No server 500 error occurs
- [ ] The UI renders no sentiment pill for the affected message (safe fallback)

## Failure Criteria
- API returns 500 when reading a message with an invalid `ai_sentiment` value
- The UI crashes or throws a JavaScript error
- A pill with garbled or empty text renders in the UI

## Cleanup
- Restore the original value: `UPDATE messages SET ai_sentiment = NULL WHERE id = '{message_id}';`
