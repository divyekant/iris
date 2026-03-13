# GC-422: Get Relationship Stats — Aggregated Metrics

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: relationship-scoring
- **Tags**: contacts, relationships, scoring, strength, stats, aggregated, happy-path
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://127.0.0.1:3000
- Valid session token available

### Data
- Valid session token (source: local-db, setup: GET /api/auth/bootstrap)
- Relationship scores computed for at least 2 contacts (source: POST /api/contacts/relationships/compute)

## Steps

1. Obtain a session token
   - **Target**: `GET http://127.0.0.1:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Compute relationship scores
   - **Target**: `POST http://127.0.0.1:3000/api/contacts/relationships/compute`
   - **Input**: Header `X-Session-Token: {token}`, no body
   - **Expected**: 200 OK with `computed` >= 2

3. Fetch aggregated relationship stats
   - **Target**: `GET http://127.0.0.1:3000/api/contacts/relationships/stats`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with a JSON object containing aggregated metrics

4. Validate by-strength breakdown
   - **Target**: Response JSON from step 3
   - **Input**: Inspect the strength breakdown (e.g., `by_strength` object or equivalent top-level fields `strong`, `regular`, `weak`, `dormant`)
   - **Expected**: All four strength buckets are present with non-negative integer counts; their sum equals the total contact count

5. Validate most-active contact field
   - **Target**: Response JSON from step 3
   - **Input**: Inspect `most_active` (or equivalent field name) that identifies the top contact
   - **Expected**: Field is present; contains an `email` (valid email string) and at least one score or activity metric; email corresponds to a contact with a high activity level

6. Validate total contact count
   - **Target**: Response JSON from step 3
   - **Input**: Inspect `total` (or `computed`) field
   - **Expected**: Value matches `computed` returned from step 2

7. Cross-check stats against compute response
   - **Target**: Step 2 response vs step 3 response
   - **Input**: Compare `strong`, `regular`, `weak`, `dormant` counts from both responses
   - **Expected**: Category counts are consistent between the compute response and the stats response (both reflect the same computed state)

## Success Criteria
- [ ] Response status is 200
- [ ] By-strength breakdown is present with all four strength labels
- [ ] Sum of strength counts equals total contact count
- [ ] `most_active` field is present and contains a valid email
- [ ] Total count matches the `computed` value from the prior compute call
- [ ] Category counts in stats are consistent with the compute response

## Failure Criteria
- Non-200 status code
- Any strength category missing from the breakdown
- Strength counts do not sum to total
- `most_active` field is absent or contains an invalid/empty email
- Stats show different category counts than the compute response without an intervening compute

## Notes
The stats endpoint provides a snapshot summary suitable for dashboard display. The `most_active` contact is determined by whichever contact has the highest combined interaction volume (total_sent + total_received) or highest `overall_score` — accept either interpretation. If no contacts are scored, the endpoint should return zeros rather than an error.
