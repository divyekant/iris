# GC-144: Sentiment Absent for Unprocessed Messages

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: sentiment-analysis
- **Tags**: sentiment-analysis, api, unprocessed, null, edge
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions

### Environment
- App running at http://127.0.0.1:3000
- At least one email account synced

### Data
- At least one message that has NOT been processed by the AI pipeline (e.g., synced while Ollama was unavailable, or newly synced and the job queue has not yet run)
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Fetch the message list
   - **Target**: `GET /api/messages?account_id={account_id}&folder=INBOX`
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: 200 OK with a `messages` array

2. Identify a message where `ai_sentiment` is null
   - **Target**: A message object in the response where `ai_sentiment` is JSON `null`
   - **Input**: n/a
   - **Expected**: The `ai_sentiment` key is present with value `null` (not the string `"null"`, not absent)

3. Fetch the detail for that unprocessed message
   - **Target**: `GET /api/messages/{id}` for the unprocessed message ID
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: 200 OK; `ai_sentiment` field is `null`

4. Navigate to the inbox and locate the unprocessed message row in the UI
   - **Target**: http://127.0.0.1:3000/#/
   - **Input**: n/a
   - **Expected**: No sentiment pill is rendered for this message row (the `{#if message.ai_sentiment && sentimentConfig[message.ai_sentiment]}` guard prevents rendering)

## Success Criteria
- [ ] `ai_sentiment` is serialized as JSON `null` (not omitted) for unprocessed messages in the list response
- [ ] `ai_sentiment` is `null` in the detail response for the same message
- [ ] No sentiment pill appears in the UI for this message row

## Failure Criteria
- `ai_sentiment` key is absent entirely from the JSON response (should always be present, even as null)
- A sentiment pill renders for a message with null `ai_sentiment`
- The value is an empty string `""` instead of `null`
