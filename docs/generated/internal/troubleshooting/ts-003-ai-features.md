---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
troubleshooting: ai-features
slug: ts-003-ai-features
---

# Troubleshooting: AI Features

## Overview

This guide covers issues with Ollama connectivity, model loading, AI classification, summarization, chat, writing assist, and Memories integration.

## Diagnostic Checklist

1. **Health endpoint**: `GET /api/health` returns `ollama: true/false` and `memories: true/false`.
2. **AI config**: `GET /api/config/ai` shows whether AI is enabled and which model is selected.
3. **Ollama direct check**: `curl http://localhost:11434/api/tags` should list available models.
4. **Memories direct check**: `curl http://localhost:8900/health` should return a success response.
5. **Server logs**: AI-related logs use `tracing::warn` and `tracing::debug`. Set `RUST_LOG=iris_server=debug`.

## Issue: Ollama unreachable

**Symptoms**: `/api/health` shows `ollama: false`. AI features return 502 or 503.

**Causes and resolutions**:

| Cause | Resolution |
|---|---|
| Ollama not installed or not running | Install Ollama and start it: `ollama serve` |
| Wrong URL | Check `OLLAMA_URL` environment variable (default: `http://localhost:11434`) |
| Port conflict | Another service may be using port 11434. Check with `lsof -i :11434`. |
| Docker networking | If running in Docker, use the container name or `host.docker.internal` instead of `localhost`. |
| Firewall/proxy blocking | Ensure the Iris server can reach the Ollama endpoint. |

## Issue: Model not loaded

**Symptoms**: Ollama is reachable but classification fails. Logs show generation errors.

**Causes and resolutions**:

| Cause | Resolution |
|---|---|
| Model not pulled | Run `ollama pull {model_name}` to download the model. |
| Model name mismatch | The model name in AI settings must exactly match an available model. Check `ollama list`. |
| Insufficient memory | Larger models require more RAM/VRAM. 8B models need ~8GB RAM. Check system resources. |
| Ollama loading timeout | First request after model switch may be slow while the model loads. Wait and retry. |

## Issue: AI classification not running

**Symptoms**: Messages sync but have no AI metadata (priority badges, category pills missing).

**Resolution steps**:

1. Check that `ai_enabled` is "true" in the config table (or via Settings UI).
2. Check that `ai_model` is set to a valid model name.
3. Check that Ollama is reachable: `/api/health` should show `ollama: true`.
4. Check server logs for "AI processing complete" or "Failed to parse AI response" messages.
5. If you see "Failed to parse AI response," the model is returning malformed JSON. Try a different model.

## Issue: Classification produces wrong results

**Symptoms**: Most emails are classified as "Primary" or "INFORMATIONAL" with default priority.

**Causes and resolutions**:

| Cause | Resolution |
|---|---|
| Model too small | Models under 3B parameters often struggle with structured JSON output. Use 7B+ models. |
| Model outputs markdown wrapper | The JSON extractor handles `\`\`\`json` blocks, but some models use unusual formatting. Check raw AI responses in debug logs. |
| All fields defaulting | If parsing partially fails, default values are used (intent=INFORMATIONAL, priority=0.5/normal, category=Primary). This suggests the model's JSON is malformed. |
| Body truncation losing context | Email body is truncated to 2000 chars. If key content is after the truncation point, classification may be inaccurate. |

## Issue: "Failed to parse AI response" in logs

**Symptoms**: Warning messages with the raw AI output. Messages get no AI metadata.

**Resolution**: The AI response could not be parsed as valid JSON. Common patterns:

- Model prefixes response with text before JSON: The extractor handles this by finding the first `{` and last `}`.
- Model outputs multiple JSON objects: Only the first object is extracted.
- Model outputs incomplete JSON: Missing closing braces. Try a model with better instruction following.
- Model outputs XML or YAML instead of JSON: Switch to a model that handles JSON well.

Test with: `POST /api/config/ai/test` to verify the model produces valid responses.

## Issue: Thread summarization fails

**Symptoms**: Summary panel shows loading state or error. 502 or 503 returned.

**Causes and resolutions**:

| Cause | Resolution |
|---|---|
| AI disabled or no model | Enable AI in Settings and select a model. |
| Ollama unreachable | Verify Ollama health. |
| Thread not found | The thread_id may not match any messages. Verify the thread exists. |
| Model timeout | Long threads may produce slow responses. Increase Ollama timeout or use a faster model. |

## Issue: Chat returns no context (AI says "I don't have information")

**Symptoms**: The AI chat responds that it cannot find relevant emails, even though matching emails exist.

**Causes and resolutions**:

| Cause | Resolution |
|---|---|
| Memories not running | Start the Memories server. Check health indicator in Settings. |
| Emails not stored in Memories | Emails must have been synced while Memories was running. Previously synced emails are not retroactively stored. |
| FTS5 fallback also empty | If neither semantic nor keyword search finds results, the AI has no context. Try more specific queries. |
| Query too vague | Very short or generic queries may not match. Try more specific questions. |

## Issue: Memories server unreachable

**Symptoms**: `/api/health` shows `memories: false`. Semantic search falls back to keyword. Chat uses FTS5 citations.

**Causes and resolutions**:

| Cause | Resolution |
|---|---|
| Memories not installed or not running | Start the Memories MCP server. |
| Wrong URL | Check `MEMORIES_URL` environment variable (default: `http://localhost:8900`). |
| API key mismatch | If the Memories server requires an API key, set `MEMORIES_API_KEY`. |
| Port conflict | Check `lsof -i :8900`. |

## Issue: Writing assist returns poor results

**Symptoms**: Rewritten text is low quality, off-topic, or incomplete.

**Causes and resolutions**:

| Cause | Resolution |
|---|---|
| Model too small | Larger models produce better rewrites. |
| Input too long | Very long inputs (near the 50,000 char limit) may cause the model to truncate output. |
| Wrong action selected | Ensure the correct action (rewrite/formal/casual/shorter/longer) matches the desired outcome. |

## Issue: Feedback corrections not affecting future classifications

**Symptoms**: User has corrected the same pattern multiple times but new emails are still misclassified.

**Causes and resolutions**:

| Cause | Resolution |
|---|---|
| Fewer than 2 corrections | The feedback context only includes patterns with count >= 2. Make the same correction at least twice. |
| Different exact patterns | Corrections are grouped by (field, original_value, corrected_value). If the original values differ (e.g., "Promotions" vs null), they are counted as separate patterns. |
| Model ignoring feedback context | The feedback is appended as a system prompt suffix. Some models may not follow it well. Try a different model. |

## Server Log Patterns

| Log Message | Meaning |
|---|---|
| `AI processing complete` (DEBUG) | Classification finished for a message |
| `Failed to parse AI response` (WARN) | Model returned unparseable JSON |
| `Memories upsert failed` (WARN) | Could not store email in Memories |
| `Memories search failed` (WARN) | Semantic search request failed |
| `Failed to parse Memories search response` (WARN) | Memories returned unexpected format |
