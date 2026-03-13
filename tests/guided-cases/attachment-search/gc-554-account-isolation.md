# GC-554: Attachment Search Is Isolated by Account

## Metadata
- **Type**: edge
- **Priority**: P0
- **Surface**: api
- **Flow**: attachment-search
- **Tags**: attachments, search, account-isolation, security
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- Two configured email accounts: account_A and account_B
- A message in account_A with attachment containing text "confidential pipeline details"
- account_B has no attachments with that text

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Index the attachment for the account_A message
   - **Target**: `POST http://localhost:3030/api/attachments/index/{message_id_account_a}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, `indexed_count` ≥ 1

3. Search using account_A's ID — should find results
   - **Target**: `GET http://localhost:3030/api/attachments/search?q=confidential+pipeline+details&account_id={account_a_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, at least one result

4. Search using account_B's ID — should return empty
   - **Target**: `GET http://localhost:3030/api/attachments/search?q=confidential+pipeline+details&account_id={account_b_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, `results` array is empty

## Success Criteria
- [ ] account_A search returns results
- [ ] account_B search returns empty results for the same query
- [ ] No cross-account leakage of attachment content

## Failure Criteria
- account_B search returns results from account_A
- Any 5xx error
