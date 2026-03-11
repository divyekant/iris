# GC-214: Grammar Check — Issues Include Kind, Description, and Suggestion

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: grammar-check
- **Tags**: grammar-check, api, issues, schema
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000
- At least one AI provider configured and healthy

### Data
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Submit content with known grammar issues
   - **Target**: `POST /api/ai/grammar-check`
   - **Input**: `{"content": "Their going to the store yesterday and buyed some items. The informations was very helpfull."}`
   - **Expected**: 200 OK with a non-empty `issues` array

2. Inspect each issue object
   - **Target**: Each item in the `issues` array
   - **Input**: n/a
   - **Expected**: Each issue has:
     - `kind`: string indicating issue type (e.g., "grammar", "spelling", "punctuation")
     - `description`: string explaining the issue
     - `suggestion`: string with the recommended fix

## Success Criteria
- [ ] `issues` array is non-empty
- [ ] Each issue object has a `kind` field (non-empty string)
- [ ] Each issue object has a `description` field (non-empty string)
- [ ] Each issue object has a `suggestion` field (non-empty string)
- [ ] Issue types are meaningful (e.g., "grammar", "spelling", not random strings)

## Failure Criteria
- `issues` is empty for obviously incorrect text
- Issue objects are missing `kind`, `description`, or `suggestion`
- Fields are empty strings or null
