# GC-412: Privacy report filtered by account_id returns only that account's data

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: privacy-report
- **Tags**: privacy, trackers, report, scanning
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available

### Data
- Two or more synced accounts with distinct message histories (source: prior sync or seed data)
- account_id=1 and account_id=2 both have messages; tracker counts differ between accounts

## Steps
1. Fetch the privacy report for account 1
   - **Target**: `GET /api/privacy/report?account_id=1&days=30`
   - **Input**: account_id = 1, days = 30
   - **Expected**: 200 OK with stats scoped to account 1's messages

2. Fetch the privacy report for account 2
   - **Target**: `GET /api/privacy/report?account_id=2&days=30`
   - **Input**: account_id = 2, days = 30
   - **Expected**: 200 OK with stats scoped to account 2's messages

3. Verify results are account-scoped (not aggregated)
   - **Target**: Comparison of step 1 and step 2 responses
   - **Input**: Compare `total_messages_scanned` values
   - **Expected**: The sum of both `total_messages_scanned` values equals (or is less than, due to date window) the total message count across both accounts; values are not identical unless both accounts have the same message count

4. Fetch the tracker list for account 1 and verify no account 2 messages appear
   - **Target**: `GET /api/privacy/trackers?account_id=1&limit=20`
   - **Input**: account_id = 1, limit = 20
   - **Expected**: All returned `message_id` values belong to messages associated with account 1 only

## Success Criteria
- [ ] Both report requests return 200
- [ ] `total_messages_scanned` for account 1 excludes account 2's messages
- [ ] `total_messages_scanned` for account 2 excludes account 1's messages
- [ ] Tracker detections listed for account 1 do not include message IDs from account 2
- [ ] Response shape is identical for both accounts

## Failure Criteria
- Either request returns non-200
- Both responses return identical stats (suggests account_id filter is ignored)
- Tracker list for account 1 includes message IDs from account 2
- Cross-account data leakage in any response field

## Notes
Confirms the account_id scoping is enforced across both the report and tracker-list endpoints. Data isolation between accounts is a correctness and security requirement.
