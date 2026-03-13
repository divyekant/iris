# GC-433: Negative — Update Non-Existent Pattern

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: archive-patterns
- **Tags**: archive, update, 404
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. Update pattern with invalid ID
   - **Target**: `PUT /api/ai/archive-patterns/99999`
   - **Input**: `{"enabled": false}`
   - **Expected**: 404

## Result
- **Status**: passed
- **Response**: 404
