# GC-472: Negative — Update Non-Existent Tracker

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: followup-tracking
- **Tags**: followup, update, 404
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. Update tracker with invalid ID
   - **Target**: `PUT /api/followup-tracking/nonexistent-99999`
   - **Input**: `{"note": "Updated note"}`
   - **Expected**: 404

## Result
- **Status**: passed
- **Response**: 404
