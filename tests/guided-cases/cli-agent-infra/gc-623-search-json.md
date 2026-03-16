# GC-623: `iris search "budget" --json` Returns JSON Array of Search Results

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: cli
- **Flow**: cli-agent-infra
- **Tags**: cli, search, json, output-mode, happy-path
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions

### Environment
- `iris` binary available on PATH
- Config at `~/.iris/config.toml` with valid `url` and `key`
- Iris server running at http://localhost:3030

### Data
- At least one email containing the word "budget" in subject or body (synced to the server)

## Steps
1. Run search with JSON flag
   - **Target**: `iris search "budget" --json`
   - **Expected**: Exit code 0, stdout is a JSON array

2. Verify JSON is parseable
   - **Target**: `iris search "budget" --json | python3 -m json.tool`
   - **Expected**: Exit code 0, formatted JSON with no parse errors

3. Verify result structure
   - **Target**: first element of the JSON array
   - **Expected**: Each result object contains at minimum: `id`, `subject`, `from`, `date`, and `snippet` (or `preview`) fields

4. Verify no results query exits cleanly
   - **Target**: `iris search "xyzzy_no_match_expected_99" --json`
   - **Expected**: Exit code 0, stdout is an empty JSON array `[]` (not an error or non-zero exit)

5. Verify human-readable mode (no --json) also works
   - **Target**: `iris search "budget"`
   - **Expected**: Exit code 0, tabular or list output with subject and sender per result

## Success Criteria
- [ ] `iris search "budget" --json` exits with code 0
- [ ] stdout is a valid JSON array
- [ ] Each result has `id`, `subject`, `from`, `date`, and snippet fields
- [ ] No-results query returns empty array `[]` with exit code 0
- [ ] Human-readable output (no `--json`) is also functional

## Failure Criteria
- Non-zero exit code for valid query
- stdout is not valid JSON when `--json` is used
- Result objects missing required fields
- No-results query returns non-zero exit or crashes
