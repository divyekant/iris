# GC-435: Negative — Suggest with Missing Body

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: archive-patterns
- **Tags**: archive, suggest, validation
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. Suggest with missing message_ids field
   - **Target**: `POST /api/ai/archive-patterns/suggest`
   - **Input**: `{}`
   - **Expected**: 422 with deserialization error

## Result
- **Status**: passed
- **Response**: 422 missing field `message_ids`
