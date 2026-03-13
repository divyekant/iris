# GC-322: Already-CC'd contacts not re-suggested

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: cc-suggestions
- **Tags**: cc-suggestions, deduplication, already-ccd, idempotency
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available
- AI provider configured and reachable

### Data
- A contact `bob@example.com` who co-occurs frequently with `alice@example.com` (source: seed or real inbox)

## Steps
1. POST to suggest-cc with `bob@example.com` already in the `cc` list
   - **Target**: `POST /api/ai/suggest-cc`
   - **Input**:
     ```json
     {
       "to": ["alice@example.com"],
       "cc": ["bob@example.com"],
       "subject": "Project update",
       "body_preview": "Here's the latest on Q2."
     }
     ```
   - **Expected**: 200 OK — `bob@example.com` does NOT appear in `suggestions`

2. Verify `to` recipient is also excluded from suggestions
   - **Target**: `suggestions` array
   - **Input**: All `email` fields
   - **Expected**: `alice@example.com` (the `to` recipient) is not in `suggestions`

3. Confirm other co-occurring contacts (not already in `to`/`cc`) can still appear
   - **Target**: `suggestions` array
   - **Input**: `email` fields
   - **Expected**: Contacts not already in `to` or `cc` may still appear as suggestions

## Success Criteria
- [ ] Response status is 200
- [ ] `bob@example.com` (already in `cc`) is absent from `suggestions`
- [ ] `alice@example.com` (already in `to`) is absent from `suggestions`
- [ ] Other valid co-occurring contacts may still be suggested

## Failure Criteria
- A contact already listed in `cc` appears as a suggestion
- A contact already listed in `to` appears as a suggestion
- Non-200 status code

## Notes
Suggesting contacts already on the email would be noise and could cause confusion. The deduplication check should cover both the `to` and `cc` fields provided in the request body, and should be case-insensitive to handle variations like `Bob@example.com` vs `bob@example.com`.
