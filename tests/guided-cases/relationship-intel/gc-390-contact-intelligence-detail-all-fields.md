# GC-390: Happy path — contact intelligence detail returns all fields

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: relationship-intel
- **Tags**: contacts, intelligence, relationship, detail, stats, patterns
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available
- Migration 032 (relationship_scores) applied

### Data
- At least one synced account with messages from a known contact (e.g., `alice@example.com`)
- Minimum 5 back-and-forth messages with that contact (source: IMAP sync or seed data)
- Relationship score row exists for the contact (source: scoring pipeline)

## Steps
1. Fetch detailed intelligence for a known contact
   - **Target**: `GET /api/contacts/alice@example.com/intelligence`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with JSON object containing all intelligence fields

2. Validate score breakdown
   - **Target**: `score_breakdown` object in response
   - **Input**: Inspect fields
   - **Expected**: Contains component scores (e.g., `frequency_score`, `recency_score`, `responsiveness_score`); each is a float in [0.0, 100.0]; total `relationship_score` is present

3. Validate communication stats
   - **Target**: `stats` object in response
   - **Input**: Inspect fields
   - **Expected**: Contains `total_emails` (positive int), `sent_by_you` (non-negative int), `received` (non-negative int), `avg_response_time` (float in hours, non-negative or null); `sent_by_you` + `received` <= `total_emails`

4. Validate common topics
   - **Target**: `common_topics` field in response
   - **Input**: Inspect value
   - **Expected**: Array of strings (may be empty if not yet classified); each entry is a non-empty string

5. Validate communication patterns
   - **Target**: `communication_patterns` object in response
   - **Input**: Inspect fields
   - **Expected**: Contains `most_active_day` (string day name or 0-6 integer), `most_active_hour` (0-23 integer), `avg_emails_per_week` (non-negative float)

## Success Criteria
- [ ] Response status is 200
- [ ] `relationship_score` is present and in [0.0, 100.0]
- [ ] `score_breakdown` contains at least one component score
- [ ] `stats.total_emails` is a positive integer
- [ ] `stats.sent_by_you` and `stats.received` are non-negative
- [ ] `stats.sent_by_you` + `stats.received` <= `stats.total_emails`
- [ ] `common_topics` is an array (may be empty)
- [ ] `communication_patterns.most_active_hour` is in [0, 23]
- [ ] `communication_patterns.avg_emails_per_week` is non-negative

## Failure Criteria
- Non-200 status code
- Missing `stats`, `score_breakdown`, or `communication_patterns` object
- `relationship_score` outside [0.0, 100.0]
- `sent_by_you` + `received` > `total_emails`
- `most_active_hour` outside [0, 23]
- Negative values for any count or average

## Notes
This is the primary detail endpoint test. Covers all three data groups: score breakdown, raw stats, and communication pattern metadata. All three should be present in a single response for an active contact.
