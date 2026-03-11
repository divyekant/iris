# GC-206: Draft from Intent — suggested_to Array in Response

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: draft-from-intent
- **Tags**: draft-from-intent, api, suggested-recipients, schema
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000
- At least one AI provider configured and healthy

### Data
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Generate a draft from intent that implies a recipient
   - **Target**: `POST /api/ai/draft-from-intent`
   - **Input**: `{"intent": "Email john@example.com about the quarterly review"}`
   - **Expected**: 200 OK with JSON body including `suggested_to` array

2. Verify suggested_to field structure
   - **Target**: `suggested_to` field in the response
   - **Input**: n/a
   - **Expected**: `suggested_to` is an array; if the intent contains an email address, it should appear in the array

3. Test intent without an explicit recipient
   - **Target**: `POST /api/ai/draft-from-intent`
   - **Input**: `{"intent": "Write a thank you note for the interview"}`
   - **Expected**: 200 OK; `suggested_to` is present as an empty array `[]`

## Success Criteria
- [ ] `suggested_to` is present in the response as an array
- [ ] When intent contains an email address, it appears in `suggested_to`
- [ ] When intent has no explicit recipient, `suggested_to` is an empty array

## Failure Criteria
- `suggested_to` is missing from the response
- `suggested_to` is not an array (e.g., null, string, or object)
- Explicit email addresses in the intent are not extracted
