# GC-299: Happy path — scan finds unreplied sent emails

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: followup-reminders
- **Tags**: followups, reminders, ai, scan, sent-emails
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Session token obtained via bootstrap

### Data
- At least one synced account with sent emails in the Sent folder
- At least one sent email that has received no reply (source: real inbox or seeded data)
- AI provider configured and healthy (Ollama or Anthropic)

## Steps

1. Trigger a follow-up scan on sent emails
   - **Target**: `POST /api/ai/scan-followups`
   - **Input**:
     ```bash
     curl -s -X POST http://localhost:3000/api/ai/scan-followups \
       -H "X-Session-Token: $SESSION_TOKEN" \
       -H "Content-Type: application/json"
     ```
   - **Expected**: 200 OK, response body contains `{ "scanned": <n>, "created": <m> }` where `m >= 1`

2. Retrieve the list of follow-up reminders
   - **Target**: `GET /api/followups`
   - **Input**:
     ```bash
     curl -s http://localhost:3000/api/followups \
       -H "X-Session-Token: $SESSION_TOKEN"
     ```
   - **Expected**: 200 OK, response is an array with at least one reminder object

3. Verify reminder object shape
   - **Target**: First element of the array returned in Step 2
   - **Input**: Inspect JSON fields
   - **Expected**: Each reminder contains `id`, `message_id`, `thread_id`, `subject`, `recipient`, `sent_at`, `due_at`, `status` (= `"pending"`), `urgency` (one of `low|normal|high|urgent`), `created_at`

## Success Criteria
- [ ] `POST /api/ai/scan-followups` returns 200
- [ ] `created` field in scan response is >= 1
- [ ] `GET /api/followups` returns 200 with a non-empty array
- [ ] Every returned reminder has `status` = `"pending"`
- [ ] Every returned reminder has `urgency` in `["low", "normal", "high", "urgent"]`
- [ ] Reminder references a valid sent message (`message_id` non-null)

## Failure Criteria
- `POST /api/ai/scan-followups` returns non-200
- `GET /api/followups` returns an empty array despite unreplied sent emails existing
- Reminder objects are missing required fields
- `urgency` contains a value outside the four allowed values

## Notes
Primary happy path. Confirms the full scan-and-surface pipeline: AI scans sent folder, identifies unreplied candidates, assigns urgency, persists reminders, and makes them retrievable via the list endpoint.
