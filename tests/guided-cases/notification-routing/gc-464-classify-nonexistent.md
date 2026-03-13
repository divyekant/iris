# GC-464: Negative — Classify Non-Existent Message

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: notification-routing
- **Tags**: notification, classify, 404
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. Classify non-existent message
   - **Target**: `POST /api/notifications/routing/classify`
   - **Input**: `{"message_id": "nonexistent-uuid-99999"}`
   - **Expected**: 404

## Result
- **Status**: passed
- **Response**: 404
