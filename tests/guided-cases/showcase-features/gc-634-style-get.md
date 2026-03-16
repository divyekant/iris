# GC-634: Writing Style — GET /api/style Returns Stored Traits

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: api+ui
- **Flow**: showcase-features
- **Tags**: writing-style, traits, greeting, signoff, tone, settings
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- Style analysis has already been run for `{account_id}` (GC-633 passed)
- Traits stored: greeting = "Hi", signoff = "Best", tone = "casual"

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. GET stored style traits
   - **Target**: `GET http://localhost:3030/api/style/{account_id}`
   - **Input**: Header `X-Session-Token: {token}`, path param `account_id`
   - **Expected**: 200 OK, response contains `traits` with `greeting`, `signoff`, `tone`

3. Verify values match what was analyzed
   - **Target**: `traits` from step 2
   - **Input**: compare against known analyzed values
   - **Expected**: `greeting = "Hi"`, `signoff = "Best"`, `tone = "casual"` (or values matching the analysis output from GC-633)

4. Verify Settings UI reflects traits (UI check)
   - **Target**: http://localhost:5173/#/settings — AI tab or Writing Style section
   - **Input**: navigate to Settings > AI > Writing Style
   - **Expected**: UI displays the stored greeting, sign-off, and tone. No "not analyzed yet" placeholder shown.

## Success Criteria
- [ ] GET /api/style/{account_id} returns 200 OK
- [ ] `traits.greeting` matches the analyzed value
- [ ] `traits.signoff` matches the analyzed value
- [ ] `traits.tone` matches the analyzed value
- [ ] Settings UI shows stored traits without requiring re-analysis

## Failure Criteria
- 404 if no traits stored (when they should be from GC-633)
- Traits values differ from what was analyzed
- Settings UI shows stale or empty values
