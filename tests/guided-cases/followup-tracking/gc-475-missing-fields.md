# GC-475: Negative — Create with Missing Required Fields

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: api
- **Flow**: followup-tracking
- **Tags**: followup, validation, missing-fields
- **Generated**: 2026-03-13
- **Last Executed**: 2026-03-13

## Steps
1. Create tracker with empty body
   - **Target**: `POST /api/followup-tracking`
   - **Input**: `{}`
   - **Expected**: 422 with missing field error

## Result
- **Status**: passed
- **Response**: 422, missing field `message_id`
