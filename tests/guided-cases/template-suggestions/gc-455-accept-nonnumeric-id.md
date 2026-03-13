# GC-455: Negative — Accept with Non-Numeric ID

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: template-suggestions
- **Tags**: template, accept, validation
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. Accept with non-numeric ID
   - **Target**: `POST /api/ai/template-suggestions/abc/accept`
   - **Input**: `{}`
   - **Expected**: 404

## Result
- **Status**: passed
- **Response**: 404
