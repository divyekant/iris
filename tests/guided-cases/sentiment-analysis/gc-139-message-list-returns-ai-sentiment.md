# GC-139: Message List Returns ai_sentiment Field

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: sentiment-analysis
- **Tags**: sentiment-analysis, api, message-list, happy-path
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions

### Environment
- App running at http://127.0.0.1:3000
- At least one email account synced

### Data
- At least one message has been processed by the AI classification pipeline (i.e., was synced while an AI provider was available)
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Fetch the message list for the synced account
   - **Target**: `GET /api/messages?account_id={account_id}&folder=INBOX`
   - **Input**: Valid `X-Session-Token` header
   - **Expected**: 200 OK with JSON body `{"messages": [...], "unread_count": N, "total": N}`

2. Inspect the `messages` array items
   - **Target**: Each object in the `messages` array
   - **Input**: n/a
   - **Expected**: Each object has an `ai_sentiment` key present (value may be `"positive"`, `"negative"`, `"neutral"`, `"mixed"`, or `null` for unprocessed messages)

3. Confirm at least one message has a non-null `ai_sentiment`
   - **Target**: Any message object with `ai_sentiment` not null
   - **Input**: n/a
   - **Expected**: `ai_sentiment` value is one of: `"positive"`, `"negative"`, `"neutral"`, `"mixed"`

## Success Criteria
- [ ] Response status is 200
- [ ] Response body contains a `messages` array
- [ ] Every object in `messages` has an `ai_sentiment` key (key must be present even when null)
- [ ] At least one message has `ai_sentiment` set to a valid enum value

## Failure Criteria
- Response status is not 200
- `ai_sentiment` key is absent from message objects (not serialized)
- A non-null `ai_sentiment` value is outside the set `{positive, negative, neutral, mixed}`
