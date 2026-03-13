# GC-459: Happy Path — Get Routing Config

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: notification-routing
- **Tags**: notification, routing, config, happy-path
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. Get routing config
   - **Target**: `GET /api/notifications/routing/config`
   - **Expected**: 200 with config object

## Result
- **Status**: passed
- **Response**: 200 with default config (empty categories, vip_always_push=true, urgency_threshold=high)
