# GC-640: Evolving Categories — Dynamic Category Tabs in Inbox Show Custom Categories

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api+ui
- **Flow**: showcase-features
- **Tags**: evolving-categories, inbox, tabs, ui, custom-category
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Iris web app running at http://localhost:5173
- Valid session active

### Data
- Account has at least one custom category named "Dev Notifications" (created via GC-639 or POST /api/categories)
- At least 2 messages tagged with `ai_category = "dev_notifications"` (or the custom category slug)

## Steps
1. Navigate to the Iris inbox
   - **Target**: http://localhost:5173/#/
   - **Input**: load the inbox page
   - **Expected**: inbox loads with standard tabs visible: Primary, Updates, Social, Promotions

2. Verify custom category tab appears
   - **Target**: inbox tab bar
   - **Input**: inspect tab list
   - **Expected**: a "Dev Notifications" tab (or equivalent) appears alongside the standard tabs — not replacing them

3. Click the custom category tab
   - **Target**: "Dev Notifications" tab in tab bar
   - **Input**: click the tab
   - **Expected**: inbox filters to show only messages tagged with the custom category; standard tabs remain accessible

4. Verify message count badge
   - **Target**: "Dev Notifications" tab
   - **Input**: inspect tab badge/count
   - **Expected**: badge shows ≥ 2 (matching the precondition message count)

5. Verify standard tabs still work
   - **Target**: "Primary" tab
   - **Input**: click Primary tab
   - **Expected**: inbox returns to standard primary view; custom category tab remains visible in the tab bar

## Success Criteria
- [ ] Custom category "Dev Notifications" tab appears in the inbox tab bar
- [ ] Custom tab is rendered alongside (not instead of) standard tabs
- [ ] Clicking the custom tab filters inbox to matching messages only
- [ ] Tab badge reflects correct message count
- [ ] Standard tabs remain functional after visiting the custom tab

## Failure Criteria
- Custom category tab absent from inbox UI
- Custom tab replaces or hides standard tabs
- Clicking custom tab shows all messages or no messages
- Tab badge shows 0 when matching messages exist
