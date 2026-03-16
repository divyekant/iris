# GC-621: `iris status` Returns Server Health with Version and Component Checks

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: cli
- **Flow**: cli-agent-infra
- **Tags**: cli, status, health, components, happy-path
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions

### Environment
- `iris` binary available on PATH
- Config at `~/.iris/config.toml` with valid `url` and `key` (set up via GC-620 or manually)
- Iris server running at http://localhost:3030

### Data
- None required

## Steps
1. Run status command
   - **Target**: `iris status`
   - **Expected**: Exit code 0, human-readable output showing server health

2. Verify version is displayed
   - **Target**: stdout from step 1
   - **Expected**: Output contains a version string (e.g., `version: 0.1.0` or `Iris vX.Y.Z`)

3. Verify component checks are shown
   - **Target**: stdout from step 1
   - **Expected**: Output lists at least: database status (ok/degraded), Ollama/AI provider status, overall health (healthy/degraded)

4. Run status in JSON mode
   - **Target**: `iris status --json`
   - **Expected**: Exit code 0, stdout is valid JSON with fields `version`, `status`, and `components` (or equivalent)

5. Verify JSON is parseable
   - **Target**: `iris status --json | python3 -m json.tool`
   - **Expected**: Exit code 0, formatted JSON output without parse errors

## Success Criteria
- [ ] `iris status` exits with code 0
- [ ] Human output includes version string and component health
- [ ] `iris status --json` produces valid, parseable JSON
- [ ] JSON includes `version` and component status fields
- [ ] No panic or error output on stderr

## Failure Criteria
- Non-zero exit code when server is running
- Version missing from output
- `--json` output is not valid JSON
- Component statuses absent from output
