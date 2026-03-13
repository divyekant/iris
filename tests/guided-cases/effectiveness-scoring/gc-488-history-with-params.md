# GC-488: Happy Path — History with Query Params

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: effectiveness-scoring
- **Tags**: effectiveness, history, filter
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. Get history with limit and account_id
   - **Target**: `GET /api/compose/effectiveness-history?limit=5&account_id=1`
   - **Expected**: 200 with filtered scores

## Result
- **Status**: passed
- **Response**: 200, filtered scores for account_id=1 with limit=5
