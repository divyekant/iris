# GC-216: Grammar Check — improved_content When Issues Found

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: grammar-check
- **Tags**: grammar-check, api, improved-content, correction
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000
- At least one AI provider configured and healthy

### Data
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Submit content with grammar issues
   - **Target**: `POST /api/ai/grammar-check`
   - **Input**: `{"content": "Me and him goes to the store everyday. We buyed alot of stuffs."}`
   - **Expected**: 200 OK with `improved_content` field in the response

2. Verify improved_content is corrected text
   - **Target**: `improved_content` field in the response
   - **Input**: n/a
   - **Expected**: `improved_content` is a non-empty string that fixes the grammar issues from the original content (e.g., "He and I go to the store every day. We bought a lot of stuff.")

3. Verify improved_content differs from input
   - **Target**: Compare input `content` with `improved_content`
   - **Input**: n/a
   - **Expected**: `improved_content` is different from the original input (corrections were applied)

## Success Criteria
- [ ] `improved_content` is present in the response
- [ ] `improved_content` is a non-empty string
- [ ] `improved_content` differs from the original input
- [ ] `improved_content` reads as grammatically correct text

## Failure Criteria
- `improved_content` is missing from the response
- `improved_content` is identical to the input (no corrections applied)
- `improved_content` is empty or null
- `improved_content` is more incorrect than the original
