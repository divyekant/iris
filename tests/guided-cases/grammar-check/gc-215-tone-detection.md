# GC-215: Grammar Check — Tone Detection

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: grammar-check
- **Tags**: grammar-check, api, tone, ai
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000
- At least one AI provider configured and healthy

### Data
- Session token obtained via GET /api/auth/bootstrap

## Steps

1. Check formal/professional content
   - **Target**: `POST /api/ai/grammar-check`
   - **Input**: `{"content": "Dear Mr. Johnson, I am writing to formally request a meeting to discuss the terms of our partnership agreement. Please let me know your earliest availability."}`
   - **Expected**: 200 OK; `tone` reflects the formal nature (e.g., "formal", "professional")

2. Check casual content
   - **Target**: `POST /api/ai/grammar-check`
   - **Input**: `{"content": "Hey! Just wanted to check in and see how things are going. Let's grab coffee sometime this week!"}`
   - **Expected**: 200 OK; `tone` reflects the casual nature (e.g., "casual", "friendly", "informal")

3. Verify tone is a meaningful string
   - **Target**: Both response `tone` values
   - **Input**: n/a
   - **Expected**: `tone` is a non-empty string that appropriately describes the writing style

## Success Criteria
- [ ] `tone` is a non-empty string in both responses
- [ ] Formal content produces a formal-sounding tone label
- [ ] Casual content produces a casual-sounding tone label
- [ ] The two tone values are distinct

## Failure Criteria
- `tone` is empty, null, or missing
- Both samples produce identical tone labels
- Tone label is clearly wrong (e.g., "angry" for a polite formal email)
