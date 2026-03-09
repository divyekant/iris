# GC-008: Query with no matching emails

## Metadata
- **Type**: negative
- **Priority**: P1
- **Surface**: ui+api
- **Flow**: chat-citations
- **Tags**: chat, citations, empty, no-results, honesty
- **Generated**: 2026-03-08
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured and synced
- AI provider enabled and healthy (Settings > AI shows a green status)
- Memories MCP running (or not -- behavior should be graceful either way)

### Data
- Inbox contains typical emails (newsletters, work correspondence, receipts) but nothing related to the obscure topic being queried (e.g., quantum computing, astrophysics research)

## Steps

1. Open the Iris app
   - **Target**: http://localhost:3000
   - **Expected**: App loads with TopNav visible

2. Open AI Chat panel
   - **Target**: "AI Chat" button in TopNav
   - **Expected**: ChatPanel slides in from the right

3. Ask about a topic unlikely to appear in the inbox
   - **Target**: Chat input field
   - **Input**: "Do I have any emails about quantum computing?"
   - **Expected**: User message appears, loading indicator shows

4. Observe the AI response
   - **Target**: Assistant message bubble
   - **Expected**: AI responds honestly that it does not find relevant emails about quantum computing. The response should NOT fabricate email content or cite nonexistent emails. The system prompt instructs: "If you don't have enough context to answer, say so honestly" and "Do not make up information not present in the provided emails."

5. Verify no "References:" section appears (or it is empty)
   - **Target**: Below the assistant message content
   - **Expected**: No "References:" block is rendered. The citations filtering logic (matching first 8 chars of message_id in response text) should yield zero matches since the AI has no IDs to reference.

6. Verify the API response structure
   - **Target**: Network tab > POST /api/ai/chat response body
   - **Expected**: Response JSON has `message.citations` as `null` (not an empty array), because the server sets `citations: None` when `referenced_citations` is empty

7. Try a second obscure query to confirm consistent behavior
   - **Target**: Chat input field
   - **Input**: "Show me emails about deep sea marine biology research"
   - **Expected**: Same behavior -- honest "no relevant emails" response, no citations block rendered

## Success Criteria
- [ ] AI response honestly states it cannot find relevant emails about the queried topic
- [ ] AI does not fabricate email subjects, senders, or content
- [ ] No "References:" section is rendered in the UI
- [ ] API response has `message.citations` set to `null` (not an empty array)
- [ ] No error is shown -- the chat flow completes normally with a 200 response
- [ ] The response is helpful (e.g., suggests the user check specific folders or rephrase)
- [ ] FTS5 search returns empty for terms like "quantum" and "computing" when no such emails exist
- [ ] Memories semantic search returns no relevant results (empty array or low-score matches filtered out)

## Failure Criteria
- AI fabricates email content, citing nonexistent messages about the queried topic
- A "References:" section appears with citations unrelated to the query
- API response includes a non-null citations array with entries despite no relevant emails existing
- Chat request fails with an error (4xx/5xx) instead of returning a graceful no-results response
- AI crashes or hangs when both Memories and FTS5 return empty results
