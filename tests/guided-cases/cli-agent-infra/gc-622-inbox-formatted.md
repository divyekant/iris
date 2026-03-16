# GC-622: `iris inbox --limit 5` Shows Formatted Inbox with Sender, Subject, Date

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: cli
- **Flow**: cli-agent-infra
- **Tags**: cli, inbox, list, format, happy-path
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions

### Environment
- `iris` binary available on PATH
- Config at `~/.iris/config.toml` with valid `url` and `key`
- Iris server running at http://localhost:3030

### Data
- At least 5 messages in the inbox (synced via IMAP or test fixtures)

## Steps
1. Run inbox command with limit
   - **Target**: `iris inbox --limit 5`
   - **Expected**: Exit code 0, table or list output showing at most 5 inbox entries

2. Verify required columns are present
   - **Target**: stdout from step 1
   - **Expected**: Each entry shows sender (From), subject, and date. Unread messages are visually distinguished (e.g., bold marker, asterisk, or `[unread]` label)

3. Verify limit is respected
   - **Target**: stdout from step 1
   - **Expected**: Exactly 5 (or fewer if inbox has less) entries returned — not the full inbox

4. Run inbox with JSON output
   - **Target**: `iris inbox --limit 5 --json`
   - **Expected**: Exit code 0, stdout is a JSON array with up to 5 objects each containing `id`, `from`, `subject`, `date`, `is_read` fields

5. Verify JSON structure
   - **Target**: `iris inbox --limit 5 --json | python3 -m json.tool`
   - **Expected**: Valid JSON array, no parse errors

## Success Criteria
- [ ] `iris inbox --limit 5` exits with code 0
- [ ] Output shows sender, subject, and date for each entry
- [ ] At most 5 entries returned
- [ ] Unread status is distinguishable in human output
- [ ] `--json` output is a parseable JSON array with required fields

## Failure Criteria
- Non-zero exit code
- Sender, subject, or date missing from any entry
- More than 5 entries returned despite `--limit 5`
- `--json` produces non-parseable output or missing required fields
