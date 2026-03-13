# GC-463: Happy Path — Clear Notification Digest

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: notification-routing
- **Tags**: notification, digest, clear
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. Clear digest
   - **Target**: `POST /api/notifications/digest/clear`
   - **Input**: `{}`
   - **Expected**: 200 with `{"cleared": 0}`

## Result
- **Status**: passed
- **Response**: 200 `{"cleared":0}`
