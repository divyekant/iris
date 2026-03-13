# GC-458: Negative — Delete with Negative ID

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: template-suggestions
- **Tags**: template, delete, validation
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. Delete with negative ID
   - **Target**: `DELETE /api/ai/template-suggestions/-1`
   - **Expected**: 404

## Result
- **Status**: passed
- **Response**: 404
