# GC-461: Happy Path — Classify Message for Routing

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: notification-routing
- **Tags**: notification, routing, classify
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. Classify a message
   - **Target**: `POST /api/notifications/routing/classify`
   - **Input**: `{"message_id": "<uuid>"}`
   - **Expected**: 200 with route, reason, message_id

## Notes
- message_id must be a string UUID (not integer)

## Result
- **Status**: passed
- **Response**: 200 `{"route":"digest","reason":"default routing","message_id":"..."}`
