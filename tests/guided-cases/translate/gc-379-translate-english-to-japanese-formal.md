# GC-379: Happy path — translate English text to Japanese with formal context

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: translate
- **Tags**: translate, language, ai, happy-path, japanese, formal
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000
- AI provider configured and healthy

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap)

## Steps
1. Submit English text for translation to Japanese with formal business context
   - **Target**: `POST /api/ai/translate`
   - **Input**: `{"text": "I wanted to follow up on our meeting from last week. Could you please send me the updated project timeline at your earliest convenience?", "target_language": "Japanese", "source_language": "English", "context": "email_compose", "formality": "formal"}`
   - **Expected**: 200 OK with JSON body containing `translated_text` and `target_language` fields

2. Verify the response structure
   - **Target**: Response body
   - **Input**: n/a
   - **Expected**: `translated_text` is a non-empty string in Japanese script (contains hiragana, katakana, or kanji); `target_language` equals `"Japanese"`

3. Verify the source language is reflected in the response
   - **Target**: Response body
   - **Input**: n/a
   - **Expected**: Response includes `source_language` field (value `"English"` or auto-detected equivalent); no error fields present

## Success Criteria
- [ ] Response status is 200
- [ ] `translated_text` is a non-empty string
- [ ] `translated_text` contains Japanese characters (not the original English text)
- [ ] `target_language` is present and equals `"Japanese"`
- [ ] No error fields in the response body

## Failure Criteria
- Response status is not 200
- `translated_text` is missing, empty, or identical to the input English text
- Response contains only ASCII characters (no Japanese script)
- `target_language` is absent or incorrect
