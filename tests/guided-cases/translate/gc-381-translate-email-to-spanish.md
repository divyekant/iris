# GC-381: Translate email — translate an existing message to Spanish

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: translate
- **Tags**: translate, language, ai, translate-email, spanish
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000
- AI provider configured and healthy

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap)
- At least one synced message in the local database; known `message_id` for an English-language email

## Steps
1. Retrieve a known message ID from the inbox
   - **Target**: `GET /api/messages?limit=5`
   - **Input**: n/a (header: `X-Session-Token: <token>`)
   - **Expected**: 200 OK with a list of messages; record the `id` of any English-language message

2. Translate the full email to Spanish
   - **Target**: `POST /api/ai/translate-email`
   - **Input**: `{"message_id": "<id from step 1>", "target_language": "Spanish"}`
   - **Expected**: 200 OK with JSON body containing translated email content

3. Verify the translated email structure
   - **Target**: Response body
   - **Input**: n/a
   - **Expected**: Response includes `translated_subject` and `translated_body` (or equivalent fields); both are non-empty strings; content is in Spanish (contains Spanish vocabulary/grammar, e.g., accented vowels or Spanish words)

4. Verify original message is unchanged
   - **Target**: `GET /api/messages/<id from step 1>`
   - **Input**: n/a
   - **Expected**: 200 OK; original message `subject` and `body` are unchanged (translation is non-destructive)

## Success Criteria
- [ ] Response status is 200
- [ ] Translated subject is present and non-empty
- [ ] Translated body is present and non-empty
- [ ] Translated content is in Spanish (visibly different from English original)
- [ ] Original message in the database is not modified

## Failure Criteria
- Response status is not 200
- Translated subject or body is missing or empty
- Translated content is identical to the English original
- Original message data is modified
