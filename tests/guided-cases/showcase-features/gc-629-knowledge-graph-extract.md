# GC-629: Knowledge Graph — Extract Entities from Message and Store

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: showcase-features
- **Tags**: knowledge-graph, entities, extraction, people, organizations
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- A message exists in the database with body text mentioning at least one person name, one organization, and one project/topic (e.g., "Hi Sarah, per our conversation with Acme Corp about Project Phoenix...")
- `message_id` for this message is known

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Trigger entity extraction for the message
   - **Target**: `POST http://localhost:3030/api/graph/extract/{message_id}`
   - **Input**: Header `X-Session-Token: {token}`, path param `message_id` = ID of the test message
   - **Expected**: 200 OK, response body contains `entities_extracted` count ≥ 1 and an `entities` array

3. Verify entity array structure
   - **Target**: response from step 2
   - **Input**: inspect `entities` array
   - **Expected**: each entity has `id`, `name`, `type` (one of `person`, `organization`, `project`, `topic`), and `source_message_id` matching the requested message

## Success Criteria
- [ ] POST /api/graph/extract/{message_id} returns 200 OK
- [ ] `entities_extracted` ≥ 1
- [ ] At least one entity with `type: "person"` is present
- [ ] Each entity has `id`, `name`, `type`, `source_message_id`
- [ ] `source_message_id` matches the input `message_id`
- [ ] Entities are persisted (a subsequent GET /api/graph?query={name} returns them)

## Failure Criteria
- 404 if message not found
- 422 if message body is empty or too short to extract from
- `entities` array is empty despite message containing clear named entities
- Entity records missing required fields
