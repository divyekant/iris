# GC-631: Temporal Reasoning — Natural Language Date Search Resolves to Correct Range

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: showcase-features
- **Tags**: temporal-reasoning, search, date-range, natural-language
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)
- Ollama or configured AI provider available (for NL date resolution)

### Data
- At least 3 messages received in the past 7 days
- At least 1 message received more than 14 days ago (outside "last week" range)

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. POST temporal search with natural language date phrase
   - **Target**: `POST http://localhost:3030/api/search/temporal`
   - **Input**: Header `X-Session-Token: {token}`, body:
     ```json
     { "query": "emails from last week" }
     ```
   - **Expected**: 200 OK, response contains `resolved_range` with `from` and `to` ISO timestamps, and `messages` array

3. Verify resolved date range is correct
   - **Target**: `resolved_range` from step 2
   - **Input**: compare `from` and `to` against current date (today is 2026-03-15)
   - **Expected**: `from` ≈ 2026-03-08T00:00:00Z, `to` ≈ 2026-03-14T23:59:59Z (7-day window ending yesterday)

4. Verify result messages fall within the range
   - **Target**: `messages` array from step 2
   - **Input**: inspect `date` field on each message
   - **Expected**: all message dates fall within [from, to]; no messages from older than 14 days ago appear

## Success Criteria
- [ ] POST /api/search/temporal returns 200 OK
- [ ] Response contains `resolved_range.from` and `resolved_range.to`
- [ ] `from` maps to approximately 7 days ago (start of day)
- [ ] `to` maps to approximately yesterday (end of day)
- [ ] All returned messages have dates within the resolved range
- [ ] Messages from > 14 days ago are excluded

## Failure Criteria
- 422 if query is empty or AI provider is unavailable
- `resolved_range` absent from response
- Date range is wildly wrong (e.g., spans months)
- Messages outside the resolved range appear in results
