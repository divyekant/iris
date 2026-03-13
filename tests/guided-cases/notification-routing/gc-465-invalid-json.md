# GC-465: Negative — Invalid JSON Body

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: notification-routing
- **Tags**: notification, validation, json
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. Send invalid JSON to update config
   - **Target**: `PUT /api/notifications/routing/config`
   - **Input**: `not json`
   - **Expected**: 400

## Result
- **Status**: passed
- **Response**: 400, parse error
