# GC-316: All Sub-Scores and Total Score Are in [0.0, 1.0] Range

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: relationship-priority
- **Tags**: relationship-priority, score-range, normalization, data-integrity, edge-case
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- Multiple contacts with varying interaction histories (source: seed or real inbox with diversity — some heavy communicators, some one-off contacts)
- Relationship scores computed after sync (source: POST /api/ai/relationship-priority)

## Steps

1. Obtain a session token
   - **Target**: `GET http://localhost:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Compute relationship scores for all contacts
   - **Target**: `POST http://localhost:3000/api/ai/relationship-priority`
   - **Input**: Header `X-Session-Token: {token}`, no body
   - **Expected**: 200 OK; `scored` >= 2 (need multiple contacts to test range normalization)

3. Fetch scores for the most-active contact (expected near 1.0)
   - **Target**: `GET http://localhost:3000/api/contacts/{most_frequent_sender}/relationship`
   - **Input**: Header `X-Session-Token: {token}`; use a contact with many threads
   - **Expected**: 200 OK; `frequency_score` is notably high (close to 1.0 if this is the top contact)

4. Fetch scores for a rarely-contacted address (expected near 0.0)
   - **Target**: `GET http://localhost:3000/api/contacts/{rare_contact}/relationship`
   - **Input**: Header `X-Session-Token: {token}`; use a contact with very few messages
   - **Expected**: 200 OK; most sub-scores close to 0.0

5. Validate range invariants across all fetched scores
   - **Target**: Response bodies from steps 3 and 4
   - **Input**: Check each numeric field: `score`, `frequency_score`, `recency_score`, `reply_rate_score`, `bidirectional_score`, `thread_depth_score`
   - **Expected**: Every value is a float in [0.0, 1.0] inclusive; no value exceeds 1.0 or is negative

6. Verify thread_depth_score cap
   - **Target**: A contact with very deep threads (10+ messages per thread)
   - **Input**: `thread_depth_score` field
   - **Expected**: Value is <= 1.0 even for contacts with very deep threads (cap at avg_depth / 10, max 1.0)

## Success Criteria
- [ ] All fetched `score` values are in [0.0, 1.0]
- [ ] All fetched sub-scores (`frequency_score`, `recency_score`, `reply_rate_score`, `bidirectional_score`, `thread_depth_score`) are in [0.0, 1.0]
- [ ] `thread_depth_score` does not exceed 1.0 even for very active threads
- [ ] `frequency_score` for the top contact equals approximately 1.0 (normalized against self)
- [ ] No negative values for any score field

## Failure Criteria
- Any score field returns a value > 1.0
- Any score field returns a negative value
- `frequency_score` for the top contact is not 1.0 (normalization broken)
- Server returns 500 when computing scores for edge-case contacts

## Notes
Normalization integrity is critical. `frequency_score` normalizes against the highest-frequency contact, so that contact must score 1.0. `recency_score` uses exponential decay (30-day half-life), so a message from today yields ~1.0 and a message from 30 days ago yields ~0.5. `thread_depth_score` is avg_depth / 10 capped at 1.0 — a contact averaging 15-message threads should still score 1.0, not 1.5.
