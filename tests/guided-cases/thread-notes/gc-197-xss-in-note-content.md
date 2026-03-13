# GC-197: Security — XSS Payloads in Note Content

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: thread-notes
- **Tags**: security, xss, injection, content-sanitization
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- At least one synced email thread exists (source: inbox sync)
- Note the `thread_id` of that thread

## Steps
1. Create a note with a script tag
   - **Target**: `POST /api/threads/{thread_id}/notes`
   - **Input**: `{ "content": "<script>alert('xss')</script>" }`
   - **Expected**: 201 — content stored as-is (plain text, not rendered as HTML)

2. Create a note with an event handler payload
   - **Target**: `POST /api/threads/{thread_id}/notes`
   - **Input**: `{ "content": "<img src=x onerror=alert('xss')>" }`
   - **Expected**: 201 — content stored as-is

3. Create a note with a javascript: URI
   - **Target**: `POST /api/threads/{thread_id}/notes`
   - **Input**: `{ "content": "<a href=\"javascript:alert('xss')\">click me</a>" }`
   - **Expected**: 201 — content stored as-is

4. Create a note with HTML entities and nested tags
   - **Target**: `POST /api/threads/{thread_id}/notes`
   - **Input**: `{ "content": "<div onmouseover=\"alert(1)\">hover&lt;script&gt;alert(2)&lt;/script&gt;</div>" }`
   - **Expected**: 201 — content stored as-is

5. List all notes and verify raw content preservation
   - **Target**: `GET /api/threads/{thread_id}/notes`
   - **Input**: valid `thread_id`
   - **Expected**: 200, all 4 notes present, content returned exactly as submitted (byte-for-byte)

6. Verify Content-Type header on list response
   - **Target**: Inspect response headers from step 5
   - **Input**: response headers
   - **Expected**: `Content-Type: application/json` (not `text/html`)

7. Clean up — delete all 4 notes
   - **Target**: `DELETE /api/threads/{thread_id}/notes/{id}` for each
   - **Input**: note ids from steps 1-4
   - **Expected**: 204 for each

## Success Criteria
- [ ] All XSS payloads accepted as valid note content (they are plain text)
- [ ] Content returned byte-for-byte identical to what was submitted
- [ ] Response Content-Type is `application/json`, not `text/html`
- [ ] No HTML entity encoding or sanitization applied at the API layer (UI handles rendering safely)
- [ ] No script execution possible from API response alone

## Failure Criteria
- XSS payload content is rejected (notes are plain text, all content should be accepted)
- Content is silently modified, stripped, or HTML-encoded at the API layer
- Response Content-Type is `text/html`
- API returns content in a way that could be rendered as HTML by a browser

## Notes
Thread notes are private user content stored as plain text. The API layer should store and return content verbatim — XSS prevention is the responsibility of the UI layer (NotesPanel.svelte), which should render note content as text nodes, not innerHTML. This test verifies the API does not corrupt content through unnecessary sanitization while also confirming the response format is safe (JSON, not HTML).
