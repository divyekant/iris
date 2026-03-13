# GC-391: AI summary — narrative summary generated for a contact

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: relationship-intel
- **Tags**: contacts, intelligence, relationship, ai, summary
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available
- AI provider configured and healthy (Ollama, Anthropic, or OpenAI)
- Migration 032 (relationship_scores) applied

### Data
- At least one synced account with messages from a known contact (e.g., `alice@example.com`)
- Minimum 3 messages exchanged with that contact (source: IMAP sync)

## Steps
1. Request AI-generated summary for a known contact
   - **Target**: `POST /api/contacts/alice@example.com/intelligence/ai-summary`
   - **Input**: Header `X-Session-Token: {token}`, empty body or `{}`
   - **Expected**: 200 OK with JSON object containing AI-generated narrative

2. Validate response structure
   - **Target**: Response JSON inspection
   - **Input**: Parse JSON
   - **Expected**: Contains `email` (matches `alice@example.com`), `summary` (non-empty string, at least 50 characters), `key_insights` (array of strings, 1-5 entries)

3. Validate narrative content quality
   - **Target**: `summary` string
   - **Input**: Read content
   - **Expected**: Summary references the contact, mentions communication frequency or topics, is coherent natural language prose (not JSON or raw data)

4. Validate key insights
   - **Target**: `key_insights` array
   - **Input**: Inspect each entry
   - **Expected**: Each insight is a non-empty string, actionable or descriptive (e.g., "Usually replies within 2 hours", "Discusses project planning frequently")

## Success Criteria
- [ ] Response status is 200
- [ ] `email` field matches the requested contact
- [ ] `summary` is a non-empty string (>= 50 characters)
- [ ] `key_insights` is a non-empty array
- [ ] Each key insight is a non-empty string
- [ ] Summary reads as coherent natural language prose
- [ ] No raw JSON or template placeholders appear in the summary

## Failure Criteria
- Non-200 status code
- `summary` is empty or null
- `key_insights` is missing or empty
- Summary contains raw template text like `{contact_name}` or `[INSERT]`
- Response is not valid JSON

## Notes
Tests the AI-generation pipeline for relationship intelligence. The summary should synthesize data from stats, topics, and patterns into a human-readable narrative. Key insights should be discrete, actionable observations drawn from the data.
