# GC-351: Short Email (< 20 chars) Returns "none" Immediately

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: social-engineering
- **Tags**: social-engineering, short-body, fast-path, no-ai, edge-case
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions
### Environment
- Iris server running at http://127.0.0.1:3000
- Valid session token available
- AI provider configured (not invoked for this case)

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap with `Sec-Fetch-Site: same-origin`)
- A synced message exists whose body is fewer than 20 characters, e.g. `"Hi, call me back."` (18 chars)
- The message ID is known as `{msg_id}`

## Steps
1. Obtain a session token
   - **Target**: GET http://127.0.0.1:3000/api/auth/bootstrap
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Trigger social engineering analysis on the short message
   - **Target**: POST http://127.0.0.1:3000/api/ai/detect-social-engineering
   - **Input**: Header `X-Session-Token: {token}`, body `{ "message_id": "{msg_id}" }`
   - **Expected**: 200 OK with a `SocialEngineeringResult` JSON object returned without waiting for an AI round-trip

3. Inspect the returned result
   - **Target**: (response body from step 2)
   - **Expected**:
     - `risk_level` equals `"none"`
     - `tactics` is an empty array `[]`
     - Response returns quickly (should not block on AI inference)

## Success Criteria
- [ ] Response status is 200
- [ ] `risk_level` equals `"none"`
- [ ] `tactics` is an empty array or absent
- [ ] Response is returned without triggering AI provider (no AI latency observed)

## Failure Criteria
- Response status is not 200
- `risk_level` is not `"none"`
- `tactics` is non-empty
- Response takes an unusually long time (indicating AI was invoked despite short body)
- Server returns 500
