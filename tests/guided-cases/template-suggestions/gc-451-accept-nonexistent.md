# GC-451: Negative — Accept Non-Existent Suggestion

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: template-suggestions
- **Tags**: template, accept, 404
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. Accept suggestion with invalid ID
   - **Target**: `POST /api/ai/template-suggestions/99999/accept`
   - **Input**: `{}`
   - **Expected**: 404

## Result
- **Status**: passed
- **Response**: 404
