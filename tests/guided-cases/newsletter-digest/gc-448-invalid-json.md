# GC-448: Negative — Invalid JSON Body

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: newsletter-digest
- **Tags**: newsletter, validation, json
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. Send invalid JSON
   - **Target**: `POST /api/ai/newsletter-digest`
   - **Input**: `not json`
   - **Expected**: 400

## Result
- **Status**: passed
- **Response**: 400, parse error
