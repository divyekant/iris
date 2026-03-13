# GC-447: Security — SQL Injection in Sender Filter

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: newsletter-digest
- **Tags**: newsletter, security, sql-injection
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. Preview with SQL injection in sender
   - **Target**: `POST /api/ai/newsletter-digest/preview`
   - **Input**: `{"sender": "x' OR 1=1 --"}`
   - **Expected**: 200 with empty results or error (no SQL injection)

## Result
- **Status**: failed
- **Response**: 200 with all messages returned (filter not applied for unknown sender)
- **Note**: No SQL injection occurred (parameterized queries), but filter silently ignored invalid sender, returning all messages instead of empty set
