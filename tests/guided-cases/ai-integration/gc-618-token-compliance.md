# GC-618: Token Compliance — No Hardcoded Hex Colors Visible in the UI

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: ui
- **Flow**: ai-integration
- **Tags**: design-tokens, token-compliance, colors, theming
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions
### Environment
- Iris server running at http://localhost:3000
- Browser DevTools available
### Data
- Multiple screens loaded: Inbox, ThreadView, Compose, Settings, Search, Chat

## Steps

### Step 1: Load the inbox
- **Target**: http://localhost:3000
- **Input**: Navigate to inbox
- **Expected**: Inbox renders with category tabs, thread list, top nav

### Step 2: Inspect inbox elements for hardcoded colors
- **Target**: Browser DevTools > Elements panel
- **Input**: Inspect styles on: thread rows, priority badges, category pills, top nav, account switcher
- **Expected**: All color values in computed styles reference CSS custom properties (e.g., `var(--iris-color-*)`) rather than literal hex codes (`#xxxxxx`), rgb(), or hsl() literals defined inline

### Step 3: Open a thread and inspect ThreadView
- **Target**: ThreadView elements
- **Input**: Open DevTools, inspect: thread subject, intelligence strip, trust badge, AI suggestion strip, message body wrapper (excluding sandboxed iframe contents)
- **Expected**: No hardcoded color values in element styles; all colors via design tokens

### Step 4: Open Settings and inspect all tabs
- **Target**: Settings page, all tabs
- **Input**: Open DevTools, cycle through General, Accounts, AI, API Keys tabs; inspect form elements, status pills, section headers
- **Expected**: All colors use design tokens

### Step 5: Open Compose modal and inspect
- **Target**: ComposeModal
- **Input**: Open compose; inspect toolbar, input fields, send button, AI assist dropdown
- **Expected**: All colors via design tokens; no inline hex values

### Step 6: Verify brand switching still works
- **Target**: Brand toggle (dark/light)
- **Input**: Switch brand to light, observe colors change; switch back to dark
- **Expected**: Colors visually update across all surfaces, confirming token-based theming is active

## Success Criteria
- [ ] DevTools inspection of Inbox shows no inline hex color values on core UI elements
- [ ] DevTools inspection of ThreadView shows no inline hex color values
- [ ] DevTools inspection of Settings shows no inline hex color values
- [ ] DevTools inspection of ComposeModal shows no inline hex color values
- [ ] Brand switching (dark/light) causes visible color changes across all inspected surfaces
- [ ] All priority badge colors match semantic tokens (urgent=error, high=warning, normal=success, low=text_faint)

## Failure Criteria
- Any core UI element (outside sandboxed email iframe) has a hardcoded `#xxxxxx`, `rgb(...)`, or `hsl(...)` value applied directly in CSS
- Brand switching has no visible effect on colors (tokens not wired)
- Priority badges use wrong semantic colors
