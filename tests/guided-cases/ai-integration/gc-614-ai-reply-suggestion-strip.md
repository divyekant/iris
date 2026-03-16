# GC-614: AI Reply Suggestion Strip Appears for Needs-Reply Threads

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: ui
- **Flow**: ai-integration
- **Tags**: thread-view, ai-suggestions, needs-reply, reply-strip
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions
### Environment
- Iris server running at http://localhost:3000
### Data
- At least one thread where `ai_needs_reply = true` (AI has flagged the thread as requiring a reply)
- AI has generated a reply suggestion for the thread (suggestion text is non-empty)
- The thread is unread or recently received (not already replied to)

## Steps

### Step 1: Navigate to Inbox
- **Target**: Browser at http://localhost:3000
- **Input**: Load the inbox
- **Expected**: Inbox renders; needs-reply threads visible (may show a "Reply needed" badge)

### Step 2: Open a needs-reply thread
- **Target**: Thread row flagged `ai_needs_reply = true`
- **Input**: Click thread row
- **Expected**: ThreadView opens showing the full thread

### Step 3: Observe AI suggestion strip
- **Target**: Area near the bottom of the thread view, above the compose area
- **Input**: No interaction — observe on load
- **Expected**: A suggestion strip is visible containing:
  - A short preview of the AI-generated reply text (1-2 sentences, truncated if long)
  - A "Reply with this" button
  - The strip is visually distinct from the thread messages (e.g., slightly tinted background or border)

### Step 4: Verify strip is absent for non-needs-reply thread
- **Target**: A thread where `ai_needs_reply = false`
- **Input**: Navigate to that thread
- **Expected**: No AI reply suggestion strip is visible

## Success Criteria
- [ ] AI suggestion strip appears in ThreadView for needs-reply threads
- [ ] Strip shows a non-empty preview of the suggested reply text
- [ ] "Reply with this" button is present and visible
- [ ] Strip is visually distinct from email message content
- [ ] Strip does NOT appear on threads where `ai_needs_reply = false`

## Failure Criteria
- Suggestion strip absent on needs-reply thread
- Strip shows empty or placeholder text
- "Reply with this" button missing
- Strip appears on threads that do not need a reply
- Strip overlaps or obscures thread content
