# GC-507: Extracted Data Grouped Summary

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api
- **Flow**: extracted-data
- **Tags**: extraction, summary, grouped, GET
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- At least one message with extractable data exists
- Extraction has been run on at least one message (via POST /api/extract/{id})

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response contains `token` field

2. Ensure extraction exists (extract from a message if needed)
   - **Target**: `GET http://localhost:3030/api/messages?limit=1`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with at least one message

3. Run extraction on message
   - **Target**: `POST http://localhost:3030/api/extract/{message_id}`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with extraction results

4. Get grouped summary
   - **Target**: `GET http://localhost:3030/api/extracted-data/summary`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with grouped summary object (data types as keys, counts/items as values)

## Success Criteria
- [ ] GET /api/extracted-data/summary returns 200
- [ ] Response groups extracted data by type
- [ ] Counts match the number of extracted items

## Failure Criteria
- Summary returns 500 or non-200
- Summary does not reflect recently extracted data
- Response format is not grouped by type
