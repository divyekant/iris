# GC-143: Message Detail Includes ai_sentiment

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: sentiment-analysis
- **Tags**: sentiment-analysis, api, message-detail, happy-path
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions

### Environment
- App running at http://127.0.0.1:3000
- At least one email account synced

### Data
- A message ID that has been AI-classified and has a non-null `ai_sentiment` value
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Fetch the message list to obtain a classified message ID
   - **Target**: `GET /api/messages?account_id={account_id}&folder=INBOX`
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: 200 OK; select a message ID where `ai_sentiment` is not null

2. Fetch the message detail for that ID
   - **Target**: `GET /api/messages/{id}`
   - **Input**: Valid `X-Session-Token` header; `{id}` from step 1
   - **Expected**: 200 OK with a JSON body containing a top-level `ai_sentiment` field

3. Verify the `ai_sentiment` value
   - **Target**: `ai_sentiment` field in the response body
   - **Input**: n/a
   - **Expected**: Value matches one of `"positive"`, `"negative"`, `"neutral"`, `"mixed"` and is consistent with the value returned by the list endpoint for the same message

4. Verify other AI fields are also present in the detail response
   - **Target**: `ai_intent`, `ai_priority_label`, `ai_category`, `ai_summary` fields
   - **Input**: n/a
   - **Expected**: All fields are present (may be null if not classified)

## Success Criteria
- [ ] `GET /api/messages/{id}` returns 200
- [ ] Response body contains `ai_sentiment` key at the top level
- [ ] `ai_sentiment` value is a valid enum string (not an unexpected value)
- [ ] `ai_sentiment` value matches what `GET /api/messages` returned for the same message

## Failure Criteria
- Response status is not 200
- `ai_sentiment` key is absent from the detail response body
- Value does not match the list endpoint value for the same message ID
- Value is outside the valid enum set
