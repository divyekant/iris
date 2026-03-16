# GC-613: Thread Intelligence Strip Shows Count, Action Items, Deadline; Click Expands Summary

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: ui
- **Flow**: ai-integration
- **Tags**: thread-view, intelligence-strip, ai-summary, action-items, deadlines
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions
### Environment
- Iris server running at http://localhost:3000
### Data
- At least one thread with 3+ messages already synced
- AI classification has run on the thread (ai_summary, ai_action_items, ai_deadline populated)
- Thread contains at least one identifiable action item (e.g., "Please send the report by Friday")
- Thread contains a deadline (e.g., "Due: March 20")

## Steps

### Step 1: Navigate to Inbox
- **Target**: Browser at http://localhost:3000
- **Input**: Load the inbox
- **Expected**: Inbox renders with thread list visible

### Step 2: Open a multi-message thread
- **Target**: Thread row with 3+ messages
- **Input**: Click thread row
- **Expected**: ThreadView opens; thread subject visible in header

### Step 3: Observe intelligence strip
- **Target**: Area below thread subject, above first message
- **Input**: No interaction — observe on load
- **Expected**: A compact intelligence strip is visible showing:
  - Message count (e.g., "4 messages")
  - At least one action item excerpt (e.g., "Send the report")
  - Deadline text (e.g., "Due Mar 20") if extracted
  - Strip is collapsed (summary body not shown)

### Step 4: Click the intelligence strip to expand
- **Target**: Intelligence strip / expand toggle
- **Input**: Click
- **Expected**: Strip expands to reveal full AI-generated thread summary (2-4 sentences); message count, action items, and deadline remain visible in the expanded state

### Step 5: Click again to collapse
- **Target**: Intelligence strip expand toggle (now open)
- **Input**: Click
- **Expected**: Strip collapses back to compact view; full summary hidden

## Success Criteria
- [ ] Intelligence strip renders below thread subject on load
- [ ] Message count is accurate and matches actual message count in thread
- [ ] At least one action item is shown in compact strip
- [ ] Deadline is shown when extracted by AI
- [ ] Clicking strip expands to full AI summary
- [ ] Clicking again collapses strip
- [ ] Expanded summary is readable and coherent

## Failure Criteria
- Intelligence strip absent from ThreadView
- Message count missing or incorrect
- Action items or deadline not shown despite AI data being present
- Click has no effect (strip does not expand/collapse)
- Full summary is empty or shows raw JSON
