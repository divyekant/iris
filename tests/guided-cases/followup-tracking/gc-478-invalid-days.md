# GC-478: Negative — Create with Invalid Days (0)

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: followup-tracking
- **Tags**: followup, validation, days
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. Create tracker with days=0 (below minimum)
   - **Target**: `POST /api/followup-tracking`
   - **Input**: `{"message_id": "<uuid>", "days": 0, "note": "invalid"}`
   - **Expected**: 400 (days must be 1-90)

## Result
- **Status**: passed
- **Response**: 400
