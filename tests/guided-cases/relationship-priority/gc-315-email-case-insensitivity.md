# GC-315: Email Case Insensitivity in Relationship Lookup

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: relationship-priority
- **Tags**: relationship-priority, case-insensitive, email-normalization, edge-case
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- Relationship scores have been computed and a score exists for `alice@example.com` (lowercase) (source: POST /api/ai/relationship-priority after sync)

## Steps

1. Obtain a session token
   - **Target**: `GET http://localhost:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK with JSON body containing `token` field

2. Compute relationship scores
   - **Target**: `POST http://localhost:3000/api/ai/relationship-priority`
   - **Input**: Header `X-Session-Token: {token}`, no body
   - **Expected**: 200 OK

3. Fetch relationship score using canonical lowercase address
   - **Target**: `GET http://localhost:3000/api/contacts/alice@example.com/relationship`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with RelationshipScore object; note all score values

4. Fetch relationship score using mixed-case address
   - **Target**: `GET http://localhost:3000/api/contacts/Alice@Example.COM/relationship`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with identical RelationshipScore object as step 3

5. Compare results
   - **Target**: Response bodies from steps 3 and 4
   - **Input**: Compare `score`, `frequency_score`, `recency_score`, `reply_rate_score`, `bidirectional_score`, `thread_depth_score`
   - **Expected**: All numeric fields match exactly; the `email` field in the response may be normalized to lowercase

## Success Criteria
- [ ] Both step 3 and step 4 return 200
- [ ] All score fields are identical between the two responses
- [ ] Step 4 does not return 404 (mixed case should resolve to the same record)
- [ ] Response `email` field is present and non-empty in both responses

## Failure Criteria
- Step 4 returns 404 when step 3 returns 200
- Score values differ between the two lookups
- Server returns 500 for the mixed-case address

## Notes
RFC 5321 specifies that email local parts are technically case-sensitive, but in practice virtually all mail systems treat them as case-insensitive. Iris stores and matches emails case-insensitively (LOWER() in SQL or normalization on ingest) to avoid duplicate contact records from the same sender using different capitalizations.
