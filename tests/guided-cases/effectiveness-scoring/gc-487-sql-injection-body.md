# GC-487: Security — SQL Injection in Body Content

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: effectiveness-scoring
- **Tags**: effectiveness, security, sql-injection
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. SQL injection in email body
   - **Target**: `POST /api/compose/effectiveness-score`
   - **Input**: `{"account_id": "1", "subject": "Test", "body": "Robert'); DROP TABLE messages;--", "to": "test@test.com"}`
   - **Expected**: 200 (AI processes safely, no SQL injection)

## Result
- **Status**: passed
- **Response**: 200, AI scored the content (low clarity/tone), no DB damage
