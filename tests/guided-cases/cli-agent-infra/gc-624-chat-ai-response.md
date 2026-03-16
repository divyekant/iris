# GC-624: `iris chat "summarize my unread"` Returns AI Response

## Metadata
- **Type**: positive
- **Priority**: P1
- **Surface**: cli
- **Flow**: cli-agent-infra
- **Tags**: cli, chat, ai, response, happy-path
- **Generated**: 2026-03-15
- **Last Executed**: never

## Preconditions

### Environment
- `iris` binary available on PATH
- Config at `~/.iris/config.toml` with valid `url` and `key`
- Iris server running at http://localhost:3030
- AI provider configured and reachable (Ollama or Anthropic or OpenAI)

### Data
- At least 2 unread messages in the inbox for the chat to summarize

## Steps
1. Run chat command with a natural language prompt
   - **Target**: `iris chat "summarize my unread"`
   - **Expected**: Exit code 0, stdout contains a non-empty AI-generated response

2. Verify response is substantive
   - **Target**: stdout from step 1
   - **Expected**: Response is at least 20 characters, not an error message, and references email-like content (e.g., mentions senders, subjects, or themes from unread messages)

3. Run chat in JSON mode
   - **Target**: `iris chat "summarize my unread" --json`
   - **Expected**: Exit code 0, stdout is valid JSON containing a `response` field with the AI text, and optionally `citations` or `tool_calls` fields

4. Verify JSON is parseable
   - **Target**: `iris chat "summarize my unread" --json | python3 -m json.tool`
   - **Expected**: Exit code 0, no parse errors

5. Run chat with quiet flag (suppress metadata)
   - **Target**: `iris chat "summarize my unread" --quiet`
   - **Expected**: Exit code 0, stdout contains only the AI response text with no surrounding decoration, headers, or progress indicators

## Success Criteria
- [ ] `iris chat "summarize my unread"` exits with code 0
- [ ] AI response is non-empty and substantive (>20 chars)
- [ ] `--json` produces parseable JSON with `response` field
- [ ] `--quiet` outputs only the response text
- [ ] No crash or panic on stderr

## Failure Criteria
- Non-zero exit code for reachable server and configured AI
- Empty or error-message-only response
- `--json` output is not valid JSON or missing `response` field
- Panic or unhandled error on stderr
