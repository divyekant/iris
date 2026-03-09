# GC-014: Resize chat panel

## Metadata
- **Type**: positive
- **Priority**: P2
- **Surface**: ui
- **Flow**: chat-sessions
- **Tags**: chat, panel, resize, drag, localStorage
- **Generated**: 2026-03-08
- **Last Executed**: never

## Preconditions

### Environment
- Iris app running at http://localhost:3000
- Clear any previous chat width preference: open browser DevTools console and run `localStorage.removeItem('iris-chat-width')`

### Data
- None required (source: inline)

## Steps

1. Open the AI Chat panel
   - **Target**: "AI Chat" button in TopNav
   - **Expected**: ChatPanel appears on the right side at its default width of 320px

2. Verify default width
   - **Target**: ChatPanel container element
   - **Expected**: Panel renders at approximately 320px wide. Inspect the element in DevTools to confirm `style` attribute contains `width: 320px`.

3. Drag the left edge to resize wider
   - **Target**: Left edge of the ChatPanel (1px-wide resize handle with col-resize cursor)
   - **Input**: Click and drag leftward by approximately 100px
   - **Expected**: Panel width increases as the handle is dragged left. The resize handle highlights with the primary brand color during the drag. Panel width grows toward 420px.

4. Release the drag
   - **Target**: Release mouse button
   - **Expected**: Panel stays at the new width. The resize handle returns to transparent background.

5. Verify width is persisted to localStorage
   - **Target**: Browser DevTools console
   - **Input**: Run `localStorage.getItem('iris-chat-width')`
   - **Expected**: Returns the new width value as a string (approximately "420")

6. Drag the left edge to resize narrower
   - **Target**: Left edge resize handle
   - **Input**: Click and drag rightward past the minimum boundary
   - **Expected**: Panel width decreases but does not go below 280px (minimum clamp). Panel remains usable with content not overflowing.

7. Drag the left edge to test maximum width
   - **Target**: Left edge resize handle
   - **Input**: Click and drag far to the left
   - **Expected**: Panel width increases but does not exceed 600px (maximum clamp). Main content area remains visible.

8. Close and reopen the panel to verify persistence
   - **Target**: Close button (x), then "AI Chat" button
   - **Expected**: Panel reopens at the last saved width from localStorage (not the 320px default)

## Success Criteria
- [ ] Default panel width is 320px when no localStorage value exists
- [ ] Left edge shows col-resize cursor on hover
- [ ] Dragging left edge leftward increases panel width
- [ ] Dragging left edge rightward decreases panel width
- [ ] Resize handle highlights with brand primary color during active drag
- [ ] Minimum width clamp at 280px is enforced
- [ ] Maximum width clamp at 600px is enforced
- [ ] Width is saved to localStorage under key 'iris-chat-width' on drag end
- [ ] Saved width persists across close/reopen of the panel

## Failure Criteria
- Panel does not respond to drag on the left edge
- Panel width goes below 280px or above 600px
- Width is not saved to localStorage after drag
- Resize causes layout breakage (content overflow, panel detaching from right edge)
- Cursor does not change to col-resize on hover over the left edge
- Panel flickers or jumps during resize
