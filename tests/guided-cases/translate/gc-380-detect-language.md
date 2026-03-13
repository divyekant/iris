# GC-380: Detect language — submit text and verify detected language

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: translate
- **Tags**: translate, language, ai, detect-language
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000
- AI provider configured and healthy

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap)

## Steps
1. Submit clearly French text to the detect-language endpoint
   - **Target**: `POST /api/ai/detect-language`
   - **Input**: `{"text": "Bonjour, je voudrais vous informer que la réunion est reportée à demain matin."}`
   - **Expected**: 200 OK with JSON body containing `language` field

2. Verify the detected language is French
   - **Target**: Response body
   - **Input**: n/a
   - **Expected**: `language` field value is `"French"` (or `"fr"`); response may also include `confidence` score

3. Submit clearly German text and verify detection
   - **Target**: `POST /api/ai/detect-language`
   - **Input**: `{"text": "Guten Morgen, ich wollte Sie über den aktuellen Stand des Projekts informieren."}`
   - **Expected**: 200 OK with `language` field value `"German"` (or `"de"`)

## Success Criteria
- [ ] Response status is 200 for both requests
- [ ] `language` field is present in the response
- [ ] French text is identified as French (or `"fr"`)
- [ ] German text is identified as German (or `"de"`)

## Failure Criteria
- Response status is not 200
- `language` field is missing from the response
- Either text is misidentified as English or another incorrect language
- Response body is empty or contains only an error
