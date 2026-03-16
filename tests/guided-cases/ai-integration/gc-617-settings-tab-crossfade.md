# GC-617: Settings Tab Crossfade Transition — Switching Tabs Shows Smooth Fade Animation

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: ui
- **Flow**: ai-integration
- **Tags**: settings, animation, transitions, crossfade
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions
### Environment
- Iris server running at http://localhost:3000
- Browser animations not disabled (no `prefers-reduced-motion` override)
### Data
- At least 2 settings tabs populated with content (e.g., General and Accounts)

## Steps

### Step 1: Navigate to Settings
- **Target**: Settings link in top nav
- **Input**: Click Settings
- **Expected**: Settings page opens; first tab (e.g., General) is active and its content is fully visible

### Step 2: Note the active tab and content
- **Target**: Currently active settings tab panel
- **Input**: Observe the visible content
- **Expected**: Content is fully opaque; tab is highlighted as active

### Step 3: Click a different settings tab
- **Target**: A non-active tab (e.g., Accounts)
- **Input**: Click tab
- **Expected**: A smooth fade crossfade animation occurs:
  - The current tab content fades out (opacity 1 → 0)
  - The new tab content fades in (opacity 0 → 1)
  - Animation duration is approximately 120-200ms (fast/normal per design tokens)
  - No abrupt cut or flash between tab content panels

### Step 4: Click back to the original tab
- **Target**: Original tab
- **Input**: Click
- **Expected**: Same smooth crossfade animation — new content fades in, old content fades out

### Step 5: Rapid tab switching
- **Target**: Multiple settings tabs
- **Input**: Click through 3 tabs in quick succession
- **Expected**: No visual glitch, stuck state, or overlapping content from concurrent transitions; final tab content renders cleanly

## Success Criteria
- [ ] Switching settings tabs produces a visible fade crossfade animation
- [ ] Animation completes without visual artifacts (no flicker, overlap, or stuck opacity)
- [ ] Animation duration is fast (approximately 120-200ms), not sluggish
- [ ] Content from only one tab is visible at the end of the transition
- [ ] Rapid switching resolves cleanly to the final selected tab

## Failure Criteria
- Tab switch causes an abrupt content cut with no animation
- Old and new tab content both visible simultaneously after transition completes
- Animation is excessively slow (> 500ms)
- Page flickers or goes blank during transition
- Rapid switching causes stuck or incorrect tab content
