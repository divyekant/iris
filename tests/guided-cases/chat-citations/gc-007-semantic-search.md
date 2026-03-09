# GC-007: Citation display with semantic search context

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: ui+api
- **Flow**: chat-citations
- **Tags**: chat, citations, semantic-search, memories, context
- **Generated**: 2026-03-08
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- At least one email account configured and synced
- AI provider enabled and healthy (Settings > AI shows a green status)
- Memories MCP running at localhost:8900 and reachable (Settings > Memories shows healthy indicator)
- Emails have been stored in Memories via the AI ingest pipeline (source tags: iris/{account}/messages/{id})

### Data
- Inbox contains emails about calendar events, meetings, or scheduling (e.g., "Team standup at 10am", "Project review Friday") so that a natural-language meeting query returns semantic matches
- Emails stored in Memories for semantic retrieval (source: iris/{account}/messages/{id})

## Steps

1. Open the Iris app
   - **Target**: http://localhost:3000
   - **Expected**: App loads with TopNav visible

2. Open AI Chat panel
   - **Target**: "AI Chat" button in TopNav
   - **Expected**: ChatPanel slides in from the right

3. Ask a natural language question about meetings
   - **Target**: Chat input field
   - **Input**: "Are there any meetings scheduled this week?"
   - **Expected**: User message appears, loading indicator shows while AI processes

4. Observe the AI response content
   - **Target**: Assistant message bubble
   - **Expected**: Response describes meeting-related emails with specifics (times, participants, subjects) drawn from the email context provided by semantic search

5. Verify the "References:" section
   - **Target**: Citation block below assistant message
   - **Expected**: "References:" label with envelope icon, followed by citations showing senders and subjects of the meeting-related emails

6. Verify semantic search was used (primary path)
   - **Target**: Network tab or server logs
   - **Expected**: Backend called Memories `/search` with the user query, filter prefix "iris/", limit 10. If Memories returned results, those were resolved to citations via the messages DB table. If Memories returned empty, FTS5 fallback was used instead.

7. Verify citation relevance
   - **Target**: Each citation line in the References section
   - **Expected**: Citations are topically relevant to meetings/scheduling, not random unrelated emails. The from and subject fields match actual emails in the inbox.

## Success Criteria
- [ ] AI response references specific meeting-related emails with accurate details
- [ ] "References:" section is displayed with at least one citation
- [ ] Citations are in "{from}: {subject}" format and match actual inbox emails
- [ ] Semantic search via Memories was attempted (primary search path)
- [ ] If Memories returned results, citation `message_id` values were resolved from Memories source tags (e.g., `iris/account/messages/{id}` -> `{id}`)
- [ ] If Memories was unavailable or returned no results, FTS5 fallback produced citations
- [ ] Up to 10 citations are returned (API limit)
- [ ] Citation snippets in the API response are capped at 100 characters (from `r.text.chars().take(100)`)

## Failure Criteria
- AI response is generic with no reference to specific emails despite meeting-related emails existing
- No citations appear despite relevant emails being stored in Memories and/or FTS5
- Citations reference emails completely unrelated to meetings or scheduling
- API response shows `citations: null` when relevant context was available
- Memories search errors cause the entire chat request to fail (should fall back to FTS5 gracefully)
