# GC-254: Intent Classification Covers All 7 Intent Types

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: intent-detection
- **Tags**: intent, ai, classification, enum-coverage, all-types
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Session token obtained via `GET /api/auth/bootstrap`
- AI provider configured and enabled

### Data
- Seven distinct message IDs, each containing content strongly associated with one of the 7 intent types (source: real synced emails, or injected test messages via SQLite)
- Suggested message content by type:
  - `action_request`: "Please review the attached contract and sign it by Friday EOD."
  - `question`: "Can you send me the login credentials for the staging environment?"
  - `fyi`: "FYI — the office will be closed on Monday for the public holiday."
  - `scheduling`: "Are you free for a 30-minute call this Thursday at 2pm or Friday at 10am?"
  - `sales`: "Upgrade to our Premium plan today and get 30% off for the first 3 months!"
  - `social`: "Great catching up with you at the conference! Let's stay in touch."
  - `newsletter`: "This week in tech: AI trends, new product launches, and developer tips."

## Steps

1. Obtain a session token
   - **Target**: `GET http://localhost:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with `token` field; store as `$TOKEN`

2. Detect intent for an `action_request` message
   - **Target**: `POST http://localhost:3000/api/ai/detect-intent`
   - **Input**: `{"message_id": "$ACTION_REQUEST_ID"}`; Header `x-session-token: $TOKEN`
   - **Expected**: 200 OK; `intent` is `"action_request"`

3. Detect intent for a `question` message
   - **Target**: `POST http://localhost:3000/api/ai/detect-intent`
   - **Input**: `{"message_id": "$QUESTION_ID"}`; Header `x-session-token: $TOKEN`
   - **Expected**: 200 OK; `intent` is `"question"`

4. Detect intent for an `fyi` message
   - **Target**: `POST http://localhost:3000/api/ai/detect-intent`
   - **Input**: `{"message_id": "$FYI_ID"}`; Header `x-session-token: $TOKEN`
   - **Expected**: 200 OK; `intent` is `"fyi"`

5. Detect intent for a `scheduling` message
   - **Target**: `POST http://localhost:3000/api/ai/detect-intent`
   - **Input**: `{"message_id": "$SCHEDULING_ID"}`; Header `x-session-token: $TOKEN`
   - **Expected**: 200 OK; `intent` is `"scheduling"`

6. Detect intent for a `sales` message
   - **Target**: `POST http://localhost:3000/api/ai/detect-intent`
   - **Input**: `{"message_id": "$SALES_ID"}`; Header `x-session-token: $TOKEN`
   - **Expected**: 200 OK; `intent` is `"sales"`

7. Detect intent for a `social` message
   - **Target**: `POST http://localhost:3000/api/ai/detect-intent`
   - **Input**: `{"message_id": "$SOCIAL_ID"}`; Header `x-session-token: $TOKEN`
   - **Expected**: 200 OK; `intent` is `"social"`

8. Detect intent for a `newsletter` message
   - **Target**: `POST http://localhost:3000/api/ai/detect-intent`
   - **Input**: `{"message_id": "$NEWSLETTER_ID"}`; Header `x-session-token: $TOKEN`
   - **Expected**: 200 OK; `intent` is `"newsletter"`

## Success Criteria
- [ ] All 7 POST requests return 200
- [ ] At least 5 of the 7 classifications match the expected intent type (AI classification is probabilistic; exact match for all 7 is aspirational but not guaranteed)
- [ ] No response returns an intent value outside the 7-member enum
- [ ] No response is missing the `intent` or `confidence` fields

## Failure Criteria
- Any response returns a status other than 200
- Any `intent` value is not one of the 7 valid enum members
- Fewer than 5 of 7 responses match their expected type (suggests the AI prompt is not distinguishing intents adequately)
- Any response omits the `intent` field entirely

## Notes
AI classification is non-deterministic. The 5-of-7 threshold accounts for ambiguous messages (e.g., a scheduling message could also be classified as `action_request`). The key invariant is that returned values are always members of the defined enum — not arbitrary AI-generated strings.
