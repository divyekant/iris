# GC-460: Happy Path — Update Routing Config

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: notification-routing
- **Tags**: notification, routing, config, update
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. Update routing config
   - **Target**: `PUT /api/notifications/routing/config`
   - **Input**: Full config object with push_categories, digest_categories, silent_categories, etc.
   - **Expected**: 200 with updated config

## Notes
- Must send full config shape (push_categories, digest_categories, silent_categories, push_senders, digest_interval_minutes, quiet_hours_start, quiet_hours_end, vip_always_push, urgency_threshold)
- Partial updates with `{"rules": [...]}` returns 422

## Result
- **Status**: passed
- **Response**: 200 with updated config
