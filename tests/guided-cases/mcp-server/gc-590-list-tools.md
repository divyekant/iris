# GC-590: List Available MCP Tools Returns Full Tool Catalog

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: mcp-server
- **Tags**: mcp, tools, list, GET
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3030
- Valid session token obtained from `GET /api/auth/bootstrap` (with `Sec-Fetch-Site: same-origin` header)

### Data
- None required

## Steps
1. Obtain session token
   - **Target**: `GET http://localhost:3030/api/auth/bootstrap`
   - **Input**: Header `Sec-Fetch-Site: same-origin`
   - **Expected**: 200 OK, response body contains `token` field

2. Retrieve the list of available MCP tools
   - **Target**: `GET http://localhost:3030/api/mcp/tools/list`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, `tools` array with ≥ 3 entries

3. Validate tool entry structure
   - Each tool in `tools` should include:
     - `name` (string, e.g., "search_emails", "read_email", "compose_draft")
     - `description` (string)
     - `input_schema` (JSON Schema object describing parameters)
   - **Expected**: All fields present; `input_schema` is a valid JSON Schema

## Success Criteria
- [ ] GET returns 200 OK
- [ ] `tools` array is non-empty
- [ ] Known tools (search_emails, read_email, compose_draft) are present
- [ ] Each tool has `name`, `description`, and `input_schema`

## Failure Criteria
- `tools` array is empty
- Tool entries missing schema definitions
- Known tools absent from catalog
