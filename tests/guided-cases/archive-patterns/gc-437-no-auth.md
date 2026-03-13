# GC-437: Security — No Auth Token

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: archive-patterns
- **Tags**: archive, auth, security
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. List patterns without auth token
   - **Target**: `GET /api/ai/archive-patterns` (no X-Session-Token)
   - **Expected**: 401

## Result
- **Status**: passed
- **Response**: 401
