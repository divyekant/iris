# GC-445: Security — No Auth Token

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: newsletter-digest
- **Tags**: newsletter, auth, security
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. List sources without auth
   - **Target**: `GET /api/ai/newsletter-digest/sources` (no X-Session-Token)
   - **Expected**: 401

## Result
- **Status**: passed
- **Response**: 401
