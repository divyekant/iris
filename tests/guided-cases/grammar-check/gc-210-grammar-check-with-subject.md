# GC-210: Grammar Check with Subject Included

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: grammar-check
- **Tags**: grammar-check, api, subject, comprehensive
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000
- At least one AI provider configured and healthy

### Data
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Submit content with subject for grammar check
   - **Target**: `POST /api/ai/grammar-check`
   - **Input**: `{"subject": "Urgent: Pleese Review", "content": "Hi team, please review the attached document at your earliest convenience."}`
   - **Expected**: 200 OK with JSON body containing `score`, `tone`, `issues`

2. Verify subject errors are included
   - **Target**: `issues` array in the response
   - **Input**: n/a
   - **Expected**: At least one issue references the misspelling "Pleese" in the subject line

## Success Criteria
- [ ] Response status is 200
- [ ] Issues array includes at least one issue related to the subject
- [ ] The misspelling "Pleese" is flagged
- [ ] Body content is also checked (no issues expected for the correct body)

## Failure Criteria
- Subject is ignored in the grammar check
- No issues detected for the misspelled subject
- Response is missing the standard fields
