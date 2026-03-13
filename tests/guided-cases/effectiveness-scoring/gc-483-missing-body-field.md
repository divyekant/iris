# GC-483: Negative — Score with Missing Body Field

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: effectiveness-scoring
- **Tags**: effectiveness, validation, missing-field
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. Score with missing body field
   - **Target**: `POST /api/compose/effectiveness-score`
   - **Input**: `{"account_id": "1", "subject": "Test"}`
   - **Expected**: 422

## Result
- **Status**: passed
- **Response**: 422, missing field `body`
