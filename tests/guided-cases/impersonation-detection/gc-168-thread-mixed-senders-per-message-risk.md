# GC-168: Thread with Mixed Senders Shows Per-Message Impersonation Risk

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: impersonation-detection
- **Tags**: impersonation, trust, thread, mixed-senders, per-message
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap with `Sec-Fetch-Site: same-origin`)
- A synced thread exists containing at least two messages:
  - Message A: from a legitimate sender, e.g. `alice@gmail.com` (safe, no risk)
  - Message B: from a lookalike sender, e.g. `billing@paypa1.com` (homoglyph — high risk)
- The thread ID is known as `{thread_id}`

## Steps
1. Obtain a session token
   - **Target**: GET http://127.0.0.1:3000/api/auth/bootstrap
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Fetch the thread detail
   - **Target**: GET http://127.0.0.1:3000/api/threads/{thread_id}
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with a `messages` array; each message object contains its own `impersonation_risk` field evaluated independently for that message's `from_address`

3. Verify Message A has no risk
   - **Target**: (inspect response body from step 2)
   - **Input**: locate the message sent by `alice@gmail.com`
   - **Expected**: that message's `impersonation_risk` is `null` or absent

4. Verify Message B shows high risk
   - **Target**: (inspect response body from step 2)
   - **Input**: locate the message sent by `billing@paypa1.com`
   - **Expected**: that message's `impersonation_risk` equals `{"lookalike_of": "paypal.com", "risk_level": "high"}`

## Success Criteria
- [ ] Thread response includes a `messages` array with per-message `impersonation_risk` fields
- [ ] Message A (`alice@gmail.com`): `impersonation_risk` is `null` or absent
- [ ] Message B (`billing@paypa1.com`): `impersonation_risk.risk_level` equals `"high"` and `impersonation_risk.lookalike_of` equals `"paypal.com"`
- [ ] Risk is evaluated individually per message, not aggregated at thread level

## Failure Criteria
- Thread endpoint returns non-200 status
- All messages share a single thread-level `impersonation_risk` rather than per-message fields
- Message A is falsely flagged
- Message B is not flagged
