# GC-431: Happy Path — Suggest Archives for Messages

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: archive-patterns
- **Tags**: archive, suggest, happy-path
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. Suggest archives for message IDs
   - **Target**: `POST /api/ai/archive-patterns/suggest`
   - **Input**: `{"message_ids": ["<uuid1>", "<uuid2>"]}`
   - **Expected**: 200 with `{"suggestions": [...]}`

## Notes
- message_ids must be string UUIDs (not integers)
- Initial test with integer IDs returned 422

## Result
- **Status**: passed
- **Response**: 200 `{"suggestions":[]}`
