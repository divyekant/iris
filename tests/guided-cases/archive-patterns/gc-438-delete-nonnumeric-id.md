# GC-438: Negative — Delete with Non-Numeric ID

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: archive-patterns
- **Tags**: archive, delete, validation
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. Delete with non-numeric path param
   - **Target**: `DELETE /api/ai/archive-patterns/abc`
   - **Expected**: 404 or 400

## Result
- **Status**: passed
- **Response**: 404
