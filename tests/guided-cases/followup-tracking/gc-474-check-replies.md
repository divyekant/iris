# GC-474: Happy Path — Check Replies

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: followup-tracking
- **Tags**: followup, replies, check
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. Check for replies on active trackers
   - **Target**: `POST /api/followup-tracking/check-replies`
   - **Input**: `{}`
   - **Expected**: 200 with `{"checked": N, "replies_found": N}`

## Result
- **Status**: passed
- **Response**: 200 `{"checked":0,"replies_found":0}`
