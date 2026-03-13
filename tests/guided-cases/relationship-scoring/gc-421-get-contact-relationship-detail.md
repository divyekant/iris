# GC-421: Get Specific Contact Relationship Detail with Score Breakdown

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: relationship-scoring
- **Tags**: contacts, relationships, scoring, strength, detail, breakdown, happy-path
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000
- Valid session token available

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap)
- Relationship scores computed and a known contact email (e.g., `alice@example.com`) has a score row (source: POST /api/contacts/relationships/compute)

## Steps

1. Obtain a session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Compute relationship scores to ensure data exists
   - **Target**: `POST http://127.0.0.1:3000/api/contacts/relationships/compute`
   - **Input**: Header `X-Session-Token: {token}`, no body
   - **Expected**: 200 OK with `computed` >= 1; note a contact email from the response or prior knowledge

3. Fetch the relationship detail for the known contact
   - **Target**: `GET http://127.0.0.1:3000/api/contacts/relationships/alice@example.com`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with a JSON object containing the full score breakdown

4. Validate top-level fields
   - **Target**: Response JSON from step 3
   - **Input**: Check `overall_score`, `strength_label`
   - **Expected**: `overall_score` is a float in [0.0, 1.0]; `strength_label` is one of `"strong"`, `"regular"`, `"weak"`, `"dormant"`

5. Validate strength label matches overall_score
   - **Target**: `overall_score` and `strength_label` from step 3
   - **Input**: Apply thresholds: strong >= 0.7, regular >= 0.4, weak >= 0.15, dormant < 0.15
   - **Expected**: `strength_label` matches the threshold bucket for the reported `overall_score`

6. Validate factor score breakdown
   - **Target**: Score breakdown object in response from step 3
   - **Input**: Inspect `frequency`, `recency`, `reciprocity`, `response_time`, `thread_engagement`
   - **Expected**: All five factor scores are present as floats in [0.0, 1.0]

7. Validate activity counters and timestamps
   - **Target**: Response JSON from step 3
   - **Input**: Inspect `total_sent`, `total_received`, `avg_response_time`, `last_sent`, `last_received`
   - **Expected**: `total_sent` and `total_received` are non-negative integers; `avg_response_time` is a non-negative number (seconds or ms); `last_sent` and `last_received` are ISO 8601 timestamps or null if no messages of that direction exist

## Success Criteria
- [ ] Response status is 200
- [ ] `overall_score` is in range [0.0, 1.0]
- [ ] `strength_label` is one of `strong`, `regular`, `weak`, `dormant`
- [ ] `strength_label` is consistent with the `overall_score` threshold bucket
- [ ] All five factor scores (`frequency`, `recency`, `reciprocity`, `response_time`, `thread_engagement`) are present
- [ ] Each factor score is in range [0.0, 1.0]
- [ ] `total_sent` and `total_received` are non-negative integers
- [ ] `avg_response_time` is a non-negative number
- [ ] `last_sent` and `last_received` are present (ISO 8601 or null)

## Failure Criteria
- Response status is not 200
- `overall_score` is outside [0.0, 1.0]
- `strength_label` does not match the score threshold bucket
- Any factor score is missing or outside [0.0, 1.0]
- `total_sent` or `total_received` is negative
- Server returns 500 or panics

## Notes
Core detail endpoint validating all 5 algorithm factors. The factor weights are: frequency (0.25), recency (0.25), reciprocity (0.20), response_time (0.15), thread_engagement (0.15). Verify that `overall_score` is approximately a weighted sum of the five factors — exact equality may vary by rounding, but it should be within 0.01 of the computed weighted average.
