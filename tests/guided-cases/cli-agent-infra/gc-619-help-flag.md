# GC-619: `iris --help` Shows All Subcommands and Global Flags

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: cli
- **Flow**: cli-agent-infra
- **Tags**: cli, help, subcommands, flags, smoke-test
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions

### Environment
- `iris` binary is installed and available on PATH (or via `cargo run --bin iris --`)
- No server connection required for `--help`

### Data
- None required

## Steps
1. Run the help command
   - **Target**: `iris --help`
   - **Input**: none
   - **Expected**: Exit code 0, stdout contains usage block with subcommands and global flags

2. Verify all core subcommands are listed
   - **Target**: stdout from step 1
   - **Expected**: Output includes at minimum: `init`, `status`, `inbox`, `search`, `send`, `chat`, `key`

3. Verify global flags are listed
   - **Target**: stdout from step 1
   - **Expected**: Output includes `--json` (machine-readable output) and `--quiet` (suppress non-essential output)

4. Run subcommand help for `inbox`
   - **Target**: `iris inbox --help`
   - **Expected**: Exit code 0, shows `--limit` and `--account` flags with descriptions

## Success Criteria
- [ ] `iris --help` exits with code 0
- [ ] All core subcommands (init, status, inbox, search, send, chat, key) appear in help output
- [ ] Global flags `--json` and `--quiet` are described
- [ ] `iris inbox --help` shows subcommand-specific flags
- [ ] No panic or error output on stderr

## Failure Criteria
- Non-zero exit code from `--help`
- One or more core subcommands absent from help text
- `--json` or `--quiet` not documented
- Panic or unexpected error on stderr
