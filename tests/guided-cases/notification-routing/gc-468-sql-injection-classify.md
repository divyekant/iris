# GC-468: Security — SQL Injection in Classify

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: notification-routing
- **Tags**: notification, security, sql-injection
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. SQL injection via classify message_id
   - **Target**: `POST /api/notifications/routing/classify`
   - **Input**: `{"message_id": "1 OR 1=1"}`
   - **Expected**: 404 or 422 (no SQL injection)

## Result
- **Status**: failed
- **Response**: 404 instead of 422
- **Note**: Safe -- string "1 OR 1=1" passed through parameterized query, message not found. Expected 422 but 404 is acceptable (message lookup failure, not injection).
