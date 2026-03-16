# GC-603: ThreadView Shows Reply/Reply All/Forward as Primary Actions

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: ui
- **Flow**: grouped-action-bar
- **Tags**: thread-view, action-bar, reply, forward, visual-hierarchy
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000

### Data
- At least one email thread with 2+ messages (source: local-db)

## Steps

1. Navigate to a thread
   - **Target**: http://localhost:3000/#/thread/{id} (click any thread from inbox)
   - **Expected**: ThreadView loads with message cards and action bar

2. Verify primary actions are always visible
   - **Target**: Action bar at the top of the thread view
   - **Expected**: Three buttons are always visible: Reply, Reply All, Forward. Reply is styled with gold/primary accent color. Reply All and Forward use muted text color.

3. Verify grouped dropdown buttons are visible
   - **Target**: Action bar after the primary buttons
   - **Expected**: Three dropdown trigger buttons visible: "Organize" (with chevron), "AI" (with sparkle icon + chevron), "More" (with chevron). Dividers separate each group.

4. Click Reply button
   - **Target**: Reply button (gold accent)
   - **Expected**: Reply compose form opens (inline or modal) pre-filled with the thread context

## Success Criteria
- [ ] Reply, Reply All, Forward are always visible (not hidden in dropdown)
- [ ] Reply button uses primary/gold accent color
- [ ] Organize, AI, More dropdown triggers are visible
- [ ] Visual dividers separate the button groups
- [ ] Reply opens the compose form

## Failure Criteria
- Reply is hidden inside a dropdown
- All actions are flat (no grouping)
- No visual dividers between groups
