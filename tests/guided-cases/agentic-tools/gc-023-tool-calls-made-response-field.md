# GC-023: Response JSON Includes tool_calls_made Field

## Metadata
- **Type**: edge
- **Priority**: P0
- **Surface**: api
- **Flow**: agentic-tools
- **Tags**: v11, agentic, tool_calls_made, response-format
- **Generated**: 2026-03-09
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3001
- AI provider configured and enabled
- At least one email account synced

### Data
- At least one email exists in the database (source: local-db)

## Steps

1. Send a question that will trigger tool use
   - **Target**: POST http://localhost:3001/api/ai/chat
   - **Input**: `{"session_id": "test-gc023a", "message": "Search for emails about meetings"}`
   - **Expected**: Response status 200

2. Verify tool_calls_made structure in response
   - **Target**: Response JSON `message.tool_calls_made`
   - **Expected**: Array of objects, each with fields: `name` (string), `arguments` (object), `result_preview` (string, max 200 chars)

3. Verify result_preview is truncated
   - **Target**: `tool_calls_made[0].result_preview`
   - **Expected**: String of 200 characters or fewer (result preview is capped at 200 chars)

4. Send a simple question that may NOT trigger tool use
   - **Target**: POST http://localhost:3001/api/ai/chat
   - **Input**: `{"session_id": "test-gc023b", "message": "Hello, how are you?"}`
   - **Expected**: Response status 200

5. Verify tool_calls_made is omitted when no tools were used
   - **Target**: Response JSON `message`
   - **Expected**: `tool_calls_made` field is absent from the JSON (skip_serializing_if = none)

## Success Criteria
- [ ] tool_calls_made is present and correctly structured when tools are used
- [ ] Each record has name, arguments, and result_preview fields
- [ ] result_preview is truncated to 200 chars max
- [ ] tool_calls_made is omitted from JSON when no tools are used

## Failure Criteria
- tool_calls_made has wrong structure (missing fields)
- result_preview exceeds 200 characters
- tool_calls_made is present as null/empty array when no tools are used (should be omitted entirely)

## Notes
- The `skip_serializing_if = "Option::is_none"` attribute ensures the field is omitted, not null
- result_preview uses `.chars().take(200).collect()` for safe truncation
