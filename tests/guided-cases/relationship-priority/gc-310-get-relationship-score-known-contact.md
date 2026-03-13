# GC-310: Get Relationship Score for Known Contact

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: relationship-priority
- **Tags**: relationship-priority, get-score, happy-path, sub-scores, known-contact
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- Relationship scores have been computed (run POST /api/ai/relationship-priority first) (source: prior step or test setup)
- A known contact email with prior thread history exists in the database (source: seed or real inbox, e.g. `alice@example.com`)

## Steps

1. Obtain a session token
   - **Target**: `GET http://localhost:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Compute relationship scores if not already done
   - **Target**: `POST http://localhost:3000/api/ai/relationship-priority`
   - **Input**: Header `X-Session-Token: {token}`, no body
   - **Expected**: 200 OK, `scored` >= 1

3. Fetch the relationship score for the known contact
   - **Target**: `GET http://localhost:3000/api/contacts/alice@example.com/relationship`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with a `RelationshipScore` object containing all sub-score fields

4. Validate the response shape and field values
   - **Target**: Response JSON from step 3
   - **Input**: Inspect all fields
   - **Expected**: Response contains `email`, `score`, `frequency_score`, `recency_score`, `reply_rate_score`, `bidirectional_score`, `thread_depth_score`, and `computed_at`; all numeric scores are between 0.0 and 1.0 inclusive; `email` matches the queried address; `computed_at` is a valid ISO-8601 timestamp

## Success Criteria
- [ ] Response status is 200
- [ ] `email` field matches `alice@example.com`
- [ ] All seven score fields are present: `score`, `frequency_score`, `recency_score`, `reply_rate_score`, `bidirectional_score`, `thread_depth_score`, and `computed_at`
- [ ] All numeric score fields are in range [0.0, 1.0]
- [ ] `computed_at` is a non-empty timestamp string

## Failure Criteria
- Non-200 status code
- Any score field missing from the response
- Any numeric score outside [0.0, 1.0]
- `email` field does not match queried address
- `computed_at` is null or missing

## Notes
This is the primary read path. The composite `score` is the blended aggregate; individual sub-scores allow callers to understand why a contact ranks high or low. The sub-score weights are: frequency 0.25, recency 0.25, reply_rate 0.25, bidirectional 0.15, thread_depth 0.10.
