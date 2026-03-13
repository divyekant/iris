# GC-382: Translate with informal formality level

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: translate
- **Tags**: translate, language, ai, formality, informal
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000
- AI provider configured and healthy

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap)

## Steps
1. Translate English text to French with informal formality
   - **Target**: `POST /api/ai/translate`
   - **Input**: `{"text": "Hey, are you free to grab coffee sometime this week?", "target_language": "French", "source_language": "English", "context": "casual", "formality": "informal"}`
   - **Expected**: 200 OK with `translated_text` in French

2. Translate the same text with formal formality for comparison
   - **Target**: `POST /api/ai/translate`
   - **Input**: `{"text": "Hey, are you free to grab coffee sometime this week?", "target_language": "French", "source_language": "English", "context": "email_compose", "formality": "formal"}`
   - **Expected**: 200 OK with `translated_text` in French

3. Verify the two translations differ by register
   - **Target**: Both response bodies from steps 1 and 2
   - **Input**: Compare `translated_text` values
   - **Expected**: The informal translation uses the familiar `tu` form in French; the formal translation uses the polite `vous` form; the two strings are not identical

4. Translate with neutral formality
   - **Target**: `POST /api/ai/translate`
   - **Input**: `{"text": "Please confirm your availability.", "target_language": "French", "formality": "neutral"}`
   - **Expected**: 200 OK with `translated_text` in French; no error

## Success Criteria
- [ ] All three requests return 200
- [ ] All three `translated_text` values are non-empty French strings
- [ ] Informal and formal translations are distinct strings
- [ ] Informal translation uses `tu` / formal uses `vous` (French register distinction)
- [ ] Neutral formality request succeeds without error

## Failure Criteria
- Any request returns a non-200 status
- Informal and formal translations are identical
- Translated text is empty or in the wrong language
- Server returns an error for the neutral formality value
