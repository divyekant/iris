# GC-636: Auto-Draft — POST Feedback with action=used Increases Pattern Confidence

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: showcase-features
- **Tags**: auto-draft, feedback, confidence, reinforcement, pattern
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- An auto-draft `{draft_id}` exists with `confidence` = 0.7 and `pattern_id` known (from GC-635)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Record the current pattern confidence
   - **Target**: `GET http://localhost:3030/api/auto-draft/{message_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, note `draft.confidence` value (baseline)

3. Submit feedback with action=used
   - **Target**: `POST http://localhost:3030/api/auto-draft/{draft_id}/feedback`
   - **Input**: Header `X-Session-Token: {token}`, body:
     ```json
     { "action": "used" }
     ```
   - **Expected**: 200 OK, response contains `updated_confidence` > baseline confidence

4. Verify pattern confidence increased
   - **Target**: `updated_confidence` from step 3
   - **Input**: compare against baseline from step 2
   - **Expected**: `updated_confidence` > baseline (e.g., 0.7 → 0.75 or higher)

5. Submit feedback with action=dismissed and verify no increase
   - **Target**: `POST http://localhost:3030/api/auto-draft/{draft_id}/feedback`
   - **Input**: Header `X-Session-Token: {token}`, body:
     ```json
     { "action": "dismissed" }
     ```
   - **Expected**: 200 OK, `updated_confidence` ≤ baseline (confidence decreases or holds)

## Success Criteria
- [ ] POST /api/auto-draft/{draft_id}/feedback returns 200 OK for action=used
- [ ] `updated_confidence` > baseline confidence
- [ ] Feedback with action=dismissed does not increase confidence
- [ ] Both valid action values (`used`, `dismissed`) are accepted

## Failure Criteria
- 404 if draft_id not found
- 422 if action is invalid (e.g., `action: "unknown"`)
- Confidence unchanged after action=used feedback
- Confidence increases after action=dismissed feedback
