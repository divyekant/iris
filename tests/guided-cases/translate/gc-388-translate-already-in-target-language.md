# GC-388: Edge — translate text that is already in the target language

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: translate
- **Tags**: translate, language, ai, edge-case, idempotent
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000
- AI provider configured and healthy

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap)

## Steps
1. Submit French text requesting translation to French
   - **Target**: `POST /api/ai/translate`
   - **Input**: `{"text": "Bonjour, merci de bien vouloir confirmer la réunion de demain.", "target_language": "French", "source_language": "French"}`
   - **Expected**: 200 OK; response does not error out; `translated_text` is returned

2. Verify the output is semantically equivalent to the input
   - **Target**: Response body
   - **Input**: Compare `translated_text` with the original French input
   - **Expected**: `translated_text` is either identical to the input or a very close paraphrase in French; the server handles same-language translation gracefully without crashing or returning an error

3. Omit source_language and let the server auto-detect, then request same target language
   - **Target**: `POST /api/ai/translate`
   - **Input**: `{"text": "Guten Tag, bitte bestätigen Sie den Termin.", "target_language": "German"}`
   - **Expected**: 200 OK; `translated_text` is the German input text (possibly lightly rephrased); no error

4. Check detect-language consistency for the already-target-language text
   - **Target**: `POST /api/ai/detect-language`
   - **Input**: `{"text": "Bonjour, merci de bien vouloir confirmer la réunion de demain."}`
   - **Expected**: 200 OK; `language` field reports `"French"` (confirming the text was already in French)

## Success Criteria
- [ ] Both same-language translate requests return 200
- [ ] `translated_text` is a non-empty string in the correct language (not empty, not an error message)
- [ ] detect-language confirms the original text is already in the target language
- [ ] Server does not return an error or 4xx/5xx status for this edge case

## Failure Criteria
- Server returns an error (400, 422, 500) when source and target language are the same
- `translated_text` is empty or missing
- Server returns the text in a third, unexpected language
- Response takes an abnormally long time or times out compared to cross-language translation

## Notes
This edge case validates graceful handling rather than a specific transformed output. The key requirement is that the API does not reject or crash on same-language requests. The AI may return the original text verbatim or with minor stylistic adjustments — both are acceptable outcomes.
