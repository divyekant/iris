# GC-479: Happy Path — Score Draft Effectiveness

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: effectiveness-scoring
- **Tags**: effectiveness, score, ai, happy-path
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. Score a well-formed email draft
   - **Target**: `POST /api/compose/effectiveness-score`
   - **Input**: `{"account_id": "1", "subject": "Project Update Q2", "body": "Hi team, please find attached...", "to": "team@example.com"}`
   - **Expected**: 200 with id, overall_score, breakdown (5 dimensions), feedback, tips

## Result
- **Status**: passed
- **Response**: 200, overall_score=0.8, all dimensions scored 0.6-0.9, 3 tips
