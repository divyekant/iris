# GC-484: Negative — Tips with Missing Body Field

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: effectiveness-scoring
- **Tags**: effectiveness, tips, validation
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. Get tips with missing body
   - **Target**: `POST /api/compose/effectiveness-tips`
   - **Input**: `{"subject": "Test"}`
   - **Expected**: 422

## Result
- **Status**: passed
- **Response**: 422, missing field `body`
