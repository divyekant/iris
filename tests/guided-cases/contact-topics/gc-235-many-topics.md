# GC-235: Contact With Many Topics — Large Topic Set

## Metadata
- **Type**: edge
- **Priority**: P2
- **Surface**: api
- **Flow**: contact-topics
- **Tags**: topics, volume, stress
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available
- AI provider configured and healthy

### Data
- A contact with a high volume of diverse emails (20+ messages covering many subjects)
- Ideally a contact like a team lead or project manager who discusses many topics (source: IMAP sync)

## Steps
1. Request topics for a high-volume contact
   - **Target**: `GET /api/contacts/{high-volume-email}/topics`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with topics array

2. Validate topic count and structure
   - **Target**: Response body
   - **Input**: Parse JSON
   - **Expected**: `topics` array may contain many entries; each topic is 3-6 words with count > 0

3. Verify no duplicate topics
   - **Target**: `topics` array
   - **Input**: Check for duplicate `topic` strings (case-insensitive)
   - **Expected**: No duplicate topic strings

4. Verify topic counts sum reasonably
   - **Target**: Sum of all topic counts
   - **Input**: Sum `count` fields
   - **Expected**: Individual counts are plausible relative to `total_emails`

## Success Criteria
- [ ] Response status is 200
- [ ] All topics are 3-6 words each
- [ ] No duplicate topic strings (case-insensitive)
- [ ] Each topic count is > 0
- [ ] `total_emails` matches expected message count
- [ ] Response returns within 30 seconds (AI processing time)

## Failure Criteria
- Response times out
- Duplicate topics in the array
- Topics exceed 6 words (AI output not properly constrained)
- Topic count values are 0 or negative

## Notes
The AI prompt constrains topics to 3-6 words. With many messages, the AI may identify 10+ distinct topics. This case verifies the system handles large topic sets without duplicates or malformed entries. The response may take longer than typical due to the volume of email content being analyzed.
