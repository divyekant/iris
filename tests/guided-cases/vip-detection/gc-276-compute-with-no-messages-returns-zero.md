# GC-276: Compute VIP Scores with No Messages Returns Zero Contacts Scored

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: vip-detection
- **Tags**: vip, contacts, compute, empty-inbox, edge-case
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000 — ideally a fresh instance with an empty database, or a test environment
- Valid session token obtained via `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- No messages in the `messages` table (empty inbox, or use a freshly initialized test database)
- No accounts configured, OR at least one active account but with zero synced messages

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3000/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Trigger VIP score computation on empty inbox
   - **Target**: `POST http://localhost:3000/api/contacts/vip/compute`
   - **Input**:
     ```
     curl -s -X POST http://localhost:3000/api/contacts/vip/compute \
       -H "X-Session-Token: {token}"
     ```
   - **Expected**: 200 OK, response body is `{"contacts_scored": 0, "vip_count": 0}` (or with `vip_count` reflecting any pre-existing manual VIPs if any)

3. Verify VIP list is empty
   - **Target**: `GET http://localhost:3000/api/contacts/vip`
   - **Input**:
     ```
     curl -s http://localhost:3000/api/contacts/vip \
       -H "X-Session-Token: {token}"
     ```
   - **Expected**: 200 OK, `vip_contacts` array is empty (`[]`), `threshold` is `0.6`

4. Verify server does not error on empty data
   - **Target**: `GET http://localhost:3000/api/health`
   - **Input**:
     ```
     curl -s http://localhost:3000/api/health
     ```
   - **Expected**: 200 OK, server remains healthy after compute on empty data

## Success Criteria
- [ ] POST /api/contacts/vip/compute returns 200 OK with no 500 error
- [ ] `contacts_scored` is `0` (no external contacts found in messages)
- [ ] `vip_count` is `0` (no contacts with score >= 0.6)
- [ ] GET /api/contacts/vip returns empty `vip_contacts` array
- [ ] Server health check passes after compute

## Failure Criteria
- POST returns 500 Internal Server Error on empty messages table
- `contacts_scored` is non-zero when no messages exist
- GET /api/contacts/vip returns non-empty array when no contacts were scored
- Server crashes or becomes unhealthy after compute on empty data

## Notes
The compute handler iterates over the `messages` table. If zero rows exist, the `contacts` HashMap stays empty, the scoring loop does not execute, and both `contacts_scored` and `vip_count` return 0. This verifies the algorithm handles the degenerate empty-inbox case without panicking.
