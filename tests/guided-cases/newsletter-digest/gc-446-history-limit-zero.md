# GC-446: Edge — History with limit=0

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: newsletter-digest
- **Tags**: newsletter, history, edge, limit
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. Get history with limit=0
   - **Target**: `GET /api/ai/newsletter-digest/history?limit=0`
   - **Expected**: 200 with empty digests array

## Result
- **Status**: passed
- **Response**: 200 `{"digests":[]}`
