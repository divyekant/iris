# GC-454: Security — No Auth Token

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: template-suggestions
- **Tags**: template, auth, security
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. List suggestions without auth
   - **Target**: `GET /api/ai/template-suggestions` (no X-Session-Token)
   - **Expected**: 401

## Result
- **Status**: passed
- **Response**: 401
