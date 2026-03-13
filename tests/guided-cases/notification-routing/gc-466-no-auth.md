# GC-466: Security — No Auth Token

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: notification-routing
- **Tags**: notification, auth, security
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. Get routing config without auth
   - **Target**: `GET /api/notifications/routing/config` (no X-Session-Token)
   - **Expected**: 401

## Result
- **Status**: passed
- **Response**: 401
