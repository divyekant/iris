# GC-229: Contact Topics Happy Path — Topics Returned for Known Contact

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: contact-topics
- **Tags**: topics, ai, happy-path
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available
- AI provider configured and healthy (Ollama, Anthropic, or OpenAI)

### Data
- At least one synced account with messages from a known contact (e.g., `alice@example.com`)
- Minimum 3 messages exchanged with that contact (source: IMAP sync)

## Steps
1. Request topics for a known contact
   - **Target**: `GET /api/contacts/alice@example.com/topics`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with JSON body containing `email`, `topics`, `total_emails`, `cached`

2. Validate response structure
   - **Target**: Response body
   - **Input**: Parse JSON
   - **Expected**: `email` equals `alice@example.com`, `topics` is a non-empty array, `total_emails` >= 3, `cached` is boolean

3. Validate topic entries
   - **Target**: Each entry in `topics` array
   - **Input**: Iterate entries
   - **Expected**: Each has `topic` (string, 3-6 words) and `count` (u32 > 0)

## Success Criteria
- [ ] Response status is 200
- [ ] `email` field matches requested contact
- [ ] `topics` array is non-empty
- [ ] Each topic is 3-6 words
- [ ] Each topic has a count > 0
- [ ] `total_emails` reflects actual message count with contact
- [ ] `cached` is `false` on first request (no prior cache)

## Failure Criteria
- Response status is not 200
- `topics` array is empty when messages exist and AI is enabled
- Topic strings are empty or exceed 6 words
- `total_emails` is 0 when messages exist

## Notes
This is the primary happy path. The AI analyzes all messages with the contact and returns recurring discussion topics. First call should have `cached: false`.
