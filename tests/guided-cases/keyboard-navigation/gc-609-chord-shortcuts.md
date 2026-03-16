# GC-609: Chord Shortcuts Navigate to Inbox, Sent, and Drafts

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: ui
- **Flow**: keyboard-navigation
- **Tags**: keyboard, chord-shortcuts, navigation, g-prefix, inbox, sent, drafts
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions
### Environment
- Iris app running at http://localhost:3000
- Browser focused on the Iris app (not an input field)
### Data
- At least one email account connected
- Sent and Drafts folders are accessible

## Steps

1. Press `g` then `i` to navigate to the inbox
   - **Target**: Iris app (currently on any view other than inbox, e.g., Settings)
   - **Input**: Press `g`, then within 1 second press `i`
   - **Expected**: App navigates to the Inbox view; the inbox message list is visible

2. Press `g` then `s` to navigate to the sent folder
   - **Target**: Iris app (currently on inbox)
   - **Input**: Press `g`, then `s`
   - **Expected**: App navigates to the Sent folder view; sent messages are listed

3. Press `g` then `d` to navigate to the drafts folder
   - **Target**: Iris app (currently on sent)
   - **Input**: Press `g`, then `d`
   - **Expected**: App navigates to the Drafts view; draft messages are listed

4. Verify that `g` alone does not trigger navigation
   - **Target**: Iris app
   - **Input**: Press `g`, then wait for more than 1 second without pressing a second key
   - **Expected**: No navigation occurs; the chord timeout expires silently and the app remains on the current view

5. Verify chord does not trigger in an input field
   - **Target**: Search input or compose field
   - **Input**: Click into a text input, then press `g` then `i`
   - **Expected**: Characters "gi" are typed into the input field; no navigation occurs

## Success Criteria
- [ ] `g` + `i` navigates to the Inbox
- [ ] `g` + `s` navigates to the Sent folder
- [ ] `g` + `d` navigates to the Drafts folder
- [ ] Pressing `g` alone without a follow-up key does not cause navigation
- [ ] Chord shortcuts are suppressed when keyboard focus is inside a text input

## Failure Criteria
- Any `g+x` chord performs the wrong navigation or navigates to the wrong view
- `g` alone triggers navigation
- Chord fires while typing in a compose or search input
- App does not update the active nav item to reflect the current view
