# GC-320: Thread CC participants included in suggestions

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: cc-suggestions
- **Tags**: cc-suggestions, thread-history, prior-participants, thread-id
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available
- AI provider configured and reachable

### Data
- A thread exists that has prior CC participants (e.g., `carol@example.com` was CC'd in an earlier message of the thread) (source: seed or real inbox)
- Note the `thread_id` of that thread

## Steps
1. POST to suggest-cc with a thread_id referencing a thread with prior CC participants
   - **Target**: `POST /api/ai/suggest-cc`
   - **Input**:
     ```json
     {
       "thread_id": "<thread_id_with_prior_cc>",
       "to": ["alice@example.com"],
       "cc": [],
       "subject": "Re: Project update",
       "body_preview": "Following up on the earlier discussion."
     }
     ```
   - **Expected**: 200 OK with `suggestions` that includes the prior CC participant (`carol@example.com`)

2. Verify the prior CC participant appears in suggestions
   - **Target**: `suggestions` array
   - **Input**: Email addresses from the array
   - **Expected**: `carol@example.com` (the prior CC participant) is present in the suggestions list

3. Verify the reason references thread participation
   - **Target**: `reason` field for `carol@example.com`'s suggestion
   - **Input**: String value
   - **Expected**: Reason references thread history or prior participation (e.g., "was CC'd in an earlier message in this thread")

## Success Criteria
- [ ] Response status is 200
- [ ] Prior CC participant from the thread appears in `suggestions`
- [ ] The suggestion reason references the thread context
- [ ] Providing `thread_id` enriches suggestions beyond co-occurrence alone

## Failure Criteria
- Prior CC participant absent from suggestions when they have a clear history in the thread
- `thread_id` parameter silently ignored (no difference in results compared to omitting it)
- Non-200 status code

## Notes
The `thread_id` parameter allows the backend to pull prior CC participants from earlier messages in the same thread, giving the AI richer context to reason about who should be looped in on the reply.
