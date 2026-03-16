# GC-620: `iris init` Creates Config File at ~/.iris/config.toml

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: cli
- **Flow**: cli-agent-infra
- **Tags**: cli, init, config, setup, happy-path
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions

### Environment
- `iris` binary available on PATH
- No existing `~/.iris/config.toml` (or it can be overwritten safely for the test)
- Iris server running at http://localhost:3030 with API key `iris_test` registered

### Data
- A valid API key `iris_test` exists in the server's api_keys table with sufficient permissions

## Steps
1. Remove any existing config to start clean (optional safety step)
   - **Target**: `rm -f ~/.iris/config.toml`
   - **Expected**: Command succeeds or file does not exist

2. Run `iris init`
   - **Target**: `iris init --url http://localhost:3030 --key iris_test`
   - **Expected**: Exit code 0, stdout confirms config written (e.g., "Config saved to ~/.iris/config.toml" or similar)

3. Verify config file was created
   - **Target**: `cat ~/.iris/config.toml`
   - **Expected**: File exists and contains `url = "http://localhost:3030"` and `key = "iris_test"` (or equivalent TOML structure)

4. Verify subsequent commands use the config without requiring explicit flags
   - **Target**: `iris status`
   - **Expected**: Exit code 0, returns server health — confirming the stored URL and key are used automatically

## Success Criteria
- [ ] `iris init` exits with code 0
- [ ] `~/.iris/config.toml` is created with the correct `url` and `key` values
- [ ] Config file is valid TOML
- [ ] `iris status` (no extra flags) succeeds using the written config

## Failure Criteria
- `iris init` exits with non-zero code
- Config file not created at `~/.iris/config.toml`
- Config file contains wrong URL or key
- `iris status` fails to read the written config
