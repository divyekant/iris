# GC-473: Negative — Delete Non-Existent Tracker

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: followup-tracking
- **Tags**: followup, delete, 404
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. Delete tracker with invalid ID
   - **Target**: `DELETE /api/followup-tracking/nonexistent-99999`
   - **Expected**: 404

## Result
- **Status**: passed
- **Response**: 404
