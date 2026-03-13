# GC-228: XSS Payload in Task Content — Security Validation

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: task-extraction
- **Tags**: xss, security, injection, sanitization, ai-output
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap`
- AI provider configured and enabled

### Data
- A synced email whose body contains XSS payloads embedded in task-like language, e.g.: `Please complete <script>alert('xss')</script> the report and <img src=x onerror=alert(1)> review the PR` (source: inbox sync or a message with such content)
- If no such message exists naturally, use any valid thread_id — the test focuses on how the API serializes AI output containing potential XSS

## Steps
1. Obtain a session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Extract tasks from a thread (any valid thread)
   - **Target**: `POST http://localhost:3030/api/ai/extract-tasks`
   - **Input**: Header `X-Session-Token: {token}`, Body `{"thread_id": "{thread_id}"}`
   - **Expected**: 200 OK with JSON response

3. Verify response Content-Type is application/json
   - **Target**: Response headers from step 2
   - **Input**: Inspect `Content-Type` header
   - **Expected**: `application/json` (not `text/html`)

4. Verify JSON serialization escapes special characters
   - **Target**: Response body from step 2
   - **Input**: Inspect raw response bytes
   - **Expected**: Any `<`, `>`, `"`, `'` characters in task strings are inside JSON string values (properly quoted). The response is valid JSON, not raw HTML.

5. Verify the frontend renders tasks safely
   - **Target**: ThreadView task panel in the browser
   - **Input**: Navigate to the thread and click "Extract Tasks"
   - **Expected**: Task text is rendered as plain text. No script execution, no injected HTML elements. If the AI returns `<script>alert('xss')</script>` as part of a task description, it appears as literal text, not executable HTML.

## Success Criteria
- [ ] Response Content-Type is `application/json`
- [ ] All special characters in task text are safely contained within JSON string values
- [ ] No `<script>` tags appear outside JSON string delimiters in the response
- [ ] Frontend renders task content as text, not HTML (Svelte's `{task.task}` interpolation auto-escapes)
- [ ] No JavaScript execution occurs when viewing extracted tasks

## Failure Criteria
- Response Content-Type is `text/html` with unescaped script tags
- Browser executes JavaScript from task content
- Task content is rendered as HTML rather than plain text
- Server returns 500 due to special characters in AI output

## Notes
Iris uses Axum with `Json()` response type, which serializes via serde_json — this inherently produces valid JSON with properly escaped strings. The Svelte frontend uses `{task.task}` text interpolation, which auto-escapes HTML. The XSS risk would only arise if the frontend used `{@html task.task}` (which it does not). This test confirms both layers of defense are intact.
