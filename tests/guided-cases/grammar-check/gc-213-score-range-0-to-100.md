# GC-213: Grammar Check — Score Range 0 to 100

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: grammar-check
- **Tags**: grammar-check, api, score, validation
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000
- At least one AI provider configured and healthy

### Data
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Check well-written content (expect high score)
   - **Target**: `POST /api/ai/grammar-check`
   - **Input**: `{"content": "Dear colleagues, I hope this message finds you well. I wanted to share the quarterly results, which exceeded our expectations across all metrics."}`
   - **Expected**: 200 OK; `score` is a number between 0 and 100; for well-written text, score should be high (>= 70)

2. Check poorly-written content (expect lower score)
   - **Target**: `POST /api/ai/grammar-check`
   - **Input**: `{"content": "i dont no how too rite good emails their always bad and noone reads them anyways"}`
   - **Expected**: 200 OK; `score` is a number between 0 and 100; for poor text, score should be lower than the well-written one

3. Verify score is within bounds
   - **Target**: Both response `score` values
   - **Input**: n/a
   - **Expected**: Both scores are >= 0 and <= 100; the well-written score > poorly-written score

## Success Criteria
- [ ] Both scores are numbers in the range [0, 100]
- [ ] Well-written content scores higher than poorly-written content
- [ ] Score is an integer or float (not a string)

## Failure Criteria
- Score is outside the 0-100 range
- Score is a string or non-numeric type
- Poorly-written content scores higher than well-written content
