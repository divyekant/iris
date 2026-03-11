# GC-147: Sentiment Filter in Message List (Not Implemented)

## Metadata
- **Type**: edge
- **Priority**: P2
- **Surface**: api
- **Flow**: sentiment-analysis
- **Tags**: sentiment-analysis, api, filter, not-implemented, edge
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions

### Environment
- App running at http://127.0.0.1:3000
- At least one email account synced

### Data
- Messages with at least two different `ai_sentiment` values present in the database
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Attempt to filter messages by sentiment using a query parameter
   - **Target**: `GET /api/messages?account_id={account_id}&folder=INBOX&sentiment=positive`
   - **Input**: Valid `X-Session-Token` header; `sentiment=positive` query param
   - **Expected**: 200 OK; the `sentiment` parameter is not recognized by `ListMessagesParams` — the API ignores it and returns the full unfiltered list (the struct only accepts `account_id`, `folder`, `category`, `limit`, `offset`)

2. Verify that the response includes messages of all sentiment values, not just positive
   - **Target**: The `messages` array in the response
   - **Input**: n/a
   - **Expected**: Messages with `ai_sentiment = "negative"`, `"neutral"`, or `"mixed"` are present alongside positive ones (confirming the filter had no effect)

3. Document the gap for future implementation
   - **Target**: n/a
   - **Input**: n/a
   - **Expected**: No 400 Bad Request or crash — unknown query params are silently ignored by Axum's `Query` extractor

## Success Criteria
- [ ] `GET /api/messages?sentiment=positive` returns 200 OK (no crash or error from the unknown param)
- [ ] The response is identical to `GET /api/messages` without the sentiment param (filter is a no-op)
- [ ] Multiple sentiment values are present in the response, confirming the param was ignored

## Failure Criteria
- API returns 400 or 500 due to the unrecognized `sentiment` query parameter
- The API incorrectly filters by sentiment despite having no implementation for it

## Notes
- Sentiment filtering is not currently implemented in `ListMessagesParams` (`src/api/messages.rs`). This case documents the expected behavior of the current API and serves as a baseline for when the filter is added in a future iteration.
