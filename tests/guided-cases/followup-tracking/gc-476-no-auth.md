# GC-476: Security — No Auth Token

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: followup-tracking
- **Tags**: followup, auth, security
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. List trackers without auth
   - **Target**: `GET /api/followup-tracking` (no X-Session-Token)
   - **Expected**: 401

## Result
- **Status**: passed
- **Response**: 401
