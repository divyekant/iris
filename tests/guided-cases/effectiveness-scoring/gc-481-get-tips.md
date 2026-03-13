# GC-481: Happy Path — Get Improvement Tips

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: effectiveness-scoring
- **Tags**: effectiveness, tips, ai
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. Get improvement tips for draft
   - **Target**: `POST /api/compose/effectiveness-tips`
   - **Input**: `{"subject": "Hello", "body": "Just checking in to see how things are going with the project."}`
   - **Expected**: 200 with `{"tips": [...]}`

## Result
- **Status**: passed
- **Response**: 200, 3 actionable tips
