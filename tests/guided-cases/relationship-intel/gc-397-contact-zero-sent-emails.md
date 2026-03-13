# GC-397: Edge — contact with zero sent emails shows correct one-directional stats

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: relationship-intel
- **Tags**: contacts, intelligence, relationship, one-directional, zero-sent, stats
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- A contact who has sent emails to the user but the user has never replied (e.g., a newsletter sender, mailing list, or cold outreach address)
- At least 3 received messages from this contact (source: IMAP sync); zero sent messages to this address

## Steps
1. Fetch intelligence detail for a receive-only contact
   - **Target**: `GET /api/contacts/newsletter@sender.example/intelligence`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK

2. Verify directional stats reflect receive-only relationship
   - **Target**: `stats` object in response
   - **Input**: Inspect `sent_by_you`, `received`, `total_emails`
   - **Expected**: `sent_by_you` = 0, `received` >= 3, `total_emails` = `received` (no sent messages inflate the count)

3. Verify response time stats reflect no outbound replies
   - **Target**: `stats.avg_response_time` and `communication_patterns`
   - **Input**: Inspect values
   - **Expected**: `avg_response_time` = null or 0 (you never replied), no fabricated response time data

4. Verify relationship score is lower than bidirectional contacts
   - **Target**: `relationship_score` in response
   - **Input**: Compare to a known bidirectional contact from GC-390
   - **Expected**: Score reflects diminished engagement (low responsiveness component); not necessarily 0 but lower than active bidirectional contact

## Success Criteria
- [ ] Response status is 200
- [ ] `stats.sent_by_you` = 0
- [ ] `stats.received` is a positive integer
- [ ] `stats.total_emails` = `stats.received` (no inflation from sent)
- [ ] `stats.avg_response_time` is null or 0 (no outbound replies)
- [ ] `relationship_score` is present and non-negative

## Failure Criteria
- Non-200 status code
- `sent_by_you` is non-zero when no outbound messages exist
- `avg_response_time` is a fabricated positive value when you never replied
- `total_emails` is inflated beyond `received` count
- `relationship_score` equals a fully bidirectional contact score (scoring not penalizing zero replies)

## Notes
One-directional relationships (receive-only) are common for newsletters, automated senders, and ignored cold outreach. The stats must correctly decompose the directional counts. The responsiveness component of the relationship score should reflect that the user never replied, resulting in a lower overall score than for active two-way contacts.
