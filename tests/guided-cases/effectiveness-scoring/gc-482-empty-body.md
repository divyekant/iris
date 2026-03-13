# GC-482: Negative — Score with Empty Body

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: effectiveness-scoring
- **Tags**: effectiveness, validation, empty
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. Score with empty body
   - **Target**: `POST /api/compose/effectiveness-score`
   - **Input**: `{"account_id": "1", "subject": "Test", "body": "", "to": "a@b.com"}`
   - **Expected**: 400 (body trim check)

## Result
- **Status**: passed
- **Response**: 400
