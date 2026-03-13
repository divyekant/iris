# GC-233: Contact Topics When AI Provider Is Disabled

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: contact-topics
- **Tags**: topics, ai-disabled, graceful-degradation
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available
- AI provider disabled or unreachable (stop Ollama, remove API keys for Anthropic/OpenAI)

### Data
- Messages exist from a known contact (e.g., `alice@example.com`) — at least 1 message
- No cached topics for this contact (clear `contact_topics_cache` if needed)

## Steps
1. Verify AI is unavailable
   - **Target**: `GET /api/health`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: Health response shows AI provider as unhealthy or absent

2. Request topics for a contact with messages
   - **Target**: `GET /api/contacts/alice@example.com/topics`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with empty topics (graceful degradation)

3. Validate response
   - **Target**: Response body
   - **Input**: Parse JSON
   - **Expected**: `email` equals `alice@example.com`, `topics` is empty array `[]`, `total_emails` reflects actual count

## Success Criteria
- [ ] Response status is 200 (not 500 or 503)
- [ ] `topics` array is empty `[]`
- [ ] `total_emails` still reflects actual message count (DB query works independently of AI)
- [ ] No server crash or unhandled error
- [ ] Response returns within reasonable time (no long timeout waiting for AI)

## Failure Criteria
- Response status is 500 (AI failure not handled gracefully)
- Server hangs waiting for AI provider
- `total_emails` is 0 when messages exist (DB query affected by AI state)

## Notes
When AI is disabled, the endpoint should still count messages from the DB but skip topic extraction. The UI shows the empty state with the contact's email count but no topic pills. This tests graceful degradation — AI features should not break core functionality.
