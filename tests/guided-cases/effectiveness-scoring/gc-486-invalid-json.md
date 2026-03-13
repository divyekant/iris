# GC-486: Negative — Invalid JSON

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: effectiveness-scoring
- **Tags**: effectiveness, validation, json
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. Send invalid JSON
   - **Target**: `POST /api/compose/effectiveness-score`
   - **Input**: `{invalid}`
   - **Expected**: 400

## Result
- **Status**: passed
- **Response**: 400, parse error
