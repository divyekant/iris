# GC-237: Subscription Audit — Settings UI (UI Test)

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: ui
- **Flow**: subscription-audit
- **Tags**: subscription-audit, ui, settings, skipped
- **Generated**: 2026-03-10
- **Last Executed**: 2026-03-10

## Preconditions

### Environment
- Iris running at http://127.0.0.1:3000

### Data
- At least one email account synced with recurring senders

## Steps

1. Navigate to Settings
   - **Target**: http://127.0.0.1:3000/#/settings
   - **Input**: n/a
   - **Expected**: Settings page loads

2. Locate the Subscription Audit section
   - **Target**: Settings page content
   - **Input**: n/a
   - **Expected**: A "Subscription Audit" section or tab is visible

3. View the subscription list in the UI
   - **Target**: Subscription audit section
   - **Input**: n/a
   - **Expected**: Subscriptions are displayed in a table or list with sender, count, read rate, and unsubscribe status

4. Verify UI elements match API data
   - **Target**: Compare displayed data with `GET /api/ai/subscription-audit` response
   - **Input**: n/a
   - **Expected**: All subscriptions from the API are rendered in the UI

## Success Criteria
- [ ] Subscription Audit section is accessible from Settings
- [ ] Subscriptions are displayed with sender, count, and read rate
- [ ] UI data matches the API response

## Failure Criteria
- Subscription Audit section is missing from Settings
- UI shows no data despite API returning subscriptions
- Data mismatch between UI and API

## Status
- **Skipped**: UI-only test; requires browser interaction and visual verification
