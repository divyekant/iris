# GC-471: Happy Path — Get Due/Overdue Trackers

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: followup-tracking
- **Tags**: followup, due, overdue
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. Get due trackers
   - **Target**: `GET /api/followup-tracking/due`
   - **Expected**: 200 with array (empty if no overdue trackers)

## Result
- **Status**: passed
- **Response**: 200 `[]` (tracker not yet overdue)
