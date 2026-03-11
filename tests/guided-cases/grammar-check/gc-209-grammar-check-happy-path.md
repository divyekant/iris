# GC-209: Grammar Check — Happy Path

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: grammar-check
- **Tags**: grammar-check, api, happy-path, ai
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000
- At least one AI provider configured and healthy

### Data
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Submit content for grammar check
   - **Target**: `POST /api/ai/grammar-check`
   - **Input**: `{"content": "I wanted to discussed the project timeline with you. Their are several issue that needs attention."}`
   - **Expected**: 200 OK with JSON body containing `score`, `tone`, and `issues` fields

2. Verify the response structure
   - **Target**: Response body
   - **Input**: n/a
   - **Expected**: `score` is a number (0-100); `tone` is a non-empty string; `issues` is an array of issue objects

## Success Criteria
- [ ] Response status is 200
- [ ] `score` is present and is a number between 0 and 100
- [ ] `tone` is present and is a non-empty string
- [ ] `issues` is present and is an array
- [ ] At least one issue is detected for the grammatically incorrect input

## Failure Criteria
- Response status is not 200
- `score`, `tone`, or `issues` is missing from the response
- No issues detected for obviously incorrect grammar
