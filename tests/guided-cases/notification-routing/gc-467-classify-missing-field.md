# GC-467: Negative — Classify with Missing message_id

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: notification-routing
- **Tags**: notification, classify, validation
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. Classify with missing message_id
   - **Target**: `POST /api/notifications/routing/classify`
   - **Input**: `{}`
   - **Expected**: 422

## Result
- **Status**: passed
- **Response**: 422, missing field `message_id`
