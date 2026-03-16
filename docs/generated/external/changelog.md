---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
---

# Changelog

All notable changes to Iris are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## Wave 3 — Layer 2: Agent Infrastructure (2026-03-15)

### Added
- **Iris CLI** — `iris` command-line tool for terminal access: inbox, search, send, chat, status, key management. Supports `--json` and `--quiet` output modes.
- **8 new MCP tools** — get_thread_summary, get_contact_profile, extract_tasks, extract_deadlines, chat, get_inbox_stats, manage_draft, bulk_action
- **Enhanced list_threads** — now supports filtering by unread, starred, category, date range, and sender

### Developer notes
- CLI reads config from `~/.iris/config.toml` (set up with `iris init`)
- MCP tools support both sync and async execution (AI tools run async)
- Total MCP tools: 18 (9 existing + 9 new/enhanced)

## Wave 3 — Layer 1: AI Integration (2026-03-15)

### Added
- **Thread intelligence strip** — compact bar below thread subject showing message count, action items, and deadlines at a glance. Click to expand full AI summary
- **AI reply suggestions** — for threads needing a reply, AI generates a draft preview with one-click "Reply with this"
- **Contextual chat** — opening AI Chat while viewing a thread auto-loads that thread as context

### Improved
- **Settings tab transitions** — smooth fade crossfade when switching settings tabs
- **Token compliance** — all remaining hardcoded colors replaced with design tokens

## Wave 3 — Layer 1: Keyboard-First Navigation (2026-03-15)

### Added
- **Command palette** — press Cmd+K to search and execute any command (navigate, compose, archive, change settings)
- **Mode indicator** — subtle badge in bottom-left shows current keyboard context
- **New shortcuts** — `b` to snooze, `m` to mute focused message

### Improved
- **Dynamic help overlay** — press `?` to see all shortcuts grouped by context, automatically updated as new shortcuts are added
- All existing keyboard shortcuts work exactly as before

## Wave 3 — Layer 1: Visual Hierarchy (2026-03-15)

### Improved
- **Smart badge priority** — inbox rows now show one primary badge based on importance (needs reply > deadline > intent), with overflow count for additional metadata
- **Grouped thread actions** — Reply, Reply All, and Forward always visible; Star, Snooze, Archive, Delete grouped under "Organize"; AI actions under "AI" dropdown
- **Smooth animations** — message rows collapse smoothly on archive/delete; hover actions appear with staggered fade-in

### Keyboard shortcuts
- All existing shortcuts continue to work without change
- New shortcuts: `b` for snooze, `m` for mute (coming in Chunk 3)

## [Unreleased] - 2026-03-04

### Added
- Reliable background job queue with automatic retry for AI processing
- Cross-session chat memory -- AI assistant remembers past conversations
- User preference extraction from AI feedback corrections
- Queue status API endpoint (GET /api/ai/queue-status)
- Chat memory API endpoint (GET /api/ai/chat/memory)

### Changed
- Email sync now queues AI processing instead of fire-and-forget
- AI classification uses learned user preferences for better accuracy
- Chat prompt includes past session context and user preferences

### Fixed
- AI processing no longer silently fails when Ollama is temporarily unavailable
- Memories storage retries on transient failures

## [0.1.0] - 2026-03-04

### Added

**Email Management**
- Connect Gmail and Outlook accounts via OAuth2 authentication
- Connect any email provider via manual IMAP configuration
- Unified inbox showing messages from all connected accounts, sorted by date
- Thread view displaying full email conversations with HTML rendering in a secure sandboxed iframe
- Compose, reply, reply all, and forward emails with full SMTP support
- Auto-saving drafts with draft management (create, edit, delete)
- Batch message actions: archive, delete, mark read/unread, star/unstar
- Category tabs to filter inbox by Primary, Updates, Social, and Promotions
- Account switcher in the sidebar to filter messages by account
- Traditional and messaging view mode toggle
- Real-time inbox updates via IMAP IDLE and WebSocket notifications

**Search**
- Full-text keyword search across message subjects, senders, and bodies (powered by SQLite FTS5)
- Search result snippets with highlighted matching terms
- Filter chips for attachment presence and date ranges
- Semantic search mode using the Memories vector store for meaning-based retrieval
- Automatic fallback from semantic to keyword search when the Memories service is unavailable

**AI Classification**
- Automatic email classification on sync: intent, priority, category, and summary
- Background processing so classification does not delay message delivery
- Priority badges and category pills displayed in the inbox
- Support for any Ollama-compatible AI model
- AI feedback system to correct misclassifications and improve future accuracy
- Feedback-aware classification that learns from your correction patterns

**AI Writing Assist**
- Thread summarization: concise 2-4 sentence summaries of email conversations
- Cached summaries for instant access on subsequent views
- Writing assist in the compose modal: rewrite, formal, casual, shorter, and longer modes

**AI Chat**
- Natural language chat assistant for asking questions about your emails
- Context-aware responses with email citations linking back to source messages
- Action proposals (archive, delete, mark read, star) with confirmation before execution
- Persistent chat sessions stored in the database
- Suggestion chips for common questions

**Agent API**
- REST API for external AI agents and scripts (search, read, draft, send)
- API key management with four permission levels: read_only, draft_only, send_with_approval, autonomous
- Optional account scoping to restrict API keys to a single email account
- Bearer token authentication with SHA-256 key hashing
- Full audit logging of all agent actions

**Trust and Privacy**
- SPF, DKIM, and DMARC authentication result parsing with color-coded trust badges
- Tracking pixel detection for known email tracking services and tiny images
- Secure email rendering via DOMPurify sanitization and sandboxed iframes

**Semantic Memory**
- Integration with the Memories vector store for semantic email search
- Automatic email storage in Memories on ingest for building a searchable knowledge base
- Health indicator for Memories service connectivity in Settings

**Infrastructure**
- Docker Compose setup with Iris server and Ollama sidecar
- Health endpoint reporting database, Ollama, and Memories connectivity status
- Session-based authentication with bootstrap token for browser clients
- Environment variable configuration for all service URLs and credentials
- Graceful shutdown with SIGINT/SIGTERM handling
