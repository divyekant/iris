# GC-615: "Reply with This" Opens Compose Pre-Filled with Suggestion Text

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: ui
- **Flow**: ai-integration
- **Tags**: thread-view, compose, ai-suggestions, pre-fill, needs-reply
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions
### Environment
- Iris server running at http://localhost:3000
### Data
- At least one thread where `ai_needs_reply = true` with a non-empty AI reply suggestion
- The AI suggestion strip is visible in ThreadView (see GC-614)

## Steps

### Step 1: Open a needs-reply thread
- **Target**: Thread row with `ai_needs_reply = true`
- **Input**: Click thread row
- **Expected**: ThreadView opens; AI suggestion strip is visible with preview text and "Reply with this" button

### Step 2: Note the suggestion text
- **Target**: AI suggestion strip preview text
- **Input**: Read/note the suggestion text shown
- **Expected**: Preview text is readable and represents a plausible reply

### Step 3: Click "Reply with this"
- **Target**: "Reply with this" button in the AI suggestion strip
- **Input**: Click
- **Expected**: ComposeModal opens in reply mode with:
  - "To" field pre-filled with the original sender's email address
  - Subject pre-filled with "Re: [original subject]"
  - Body pre-filled with the full AI-generated suggestion text (not just the preview snippet)
  - Cursor positioned at the beginning or end of the pre-filled body text

### Step 4: Verify the compose body matches the suggestion
- **Target**: ComposeModal body field
- **Input**: Read the body content
- **Expected**: Body text matches the full AI suggestion (may include the full reply, not just the truncated preview shown in the strip)

### Step 5: Verify the compose is editable
- **Target**: ComposeModal body field
- **Input**: Type additional text or modify the pre-filled content
- **Expected**: Body is fully editable; changes persist until send or discard

### Step 6: Discard the compose
- **Target**: ComposeModal discard/close button
- **Input**: Click discard
- **Expected**: Modal closes; returns to ThreadView; suggestion strip still visible

## Success Criteria
- [ ] Clicking "Reply with this" opens ComposeModal
- [ ] ComposeModal is in reply mode (not new compose)
- [ ] "To" field pre-filled with original sender
- [ ] Subject pre-filled as "Re: [original subject]"
- [ ] Body pre-filled with the full AI suggestion text
- [ ] Body text is editable
- [ ] Discarding returns to ThreadView without error

## Failure Criteria
- Clicking button has no effect
- ComposeModal opens but body is empty
- ComposeModal opens as a new compose instead of reply
- "To" or subject fields are empty
- Body shows partial text only (truncated at preview length)
- Modal opens with an error state
