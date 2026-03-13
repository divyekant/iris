# GC-230: Contact Topics Cached Response Within TTL

## Metadata
- **Type**: positive
- **Priority**: P0
- **Surface**: api
- **Flow**: contact-topics
- **Tags**: topics, cache, ttl
- **Generated**: 2026-03-13
- **Last Executed**: never

## Preconditions

### Environment
- Iris server running at http://localhost:3000
- Valid session token available
- AI provider configured and healthy

### Data
- At least one synced account with messages from a known contact (e.g., `alice@example.com`)
- GC-229 executed successfully (cache populated), OR make a first request to populate cache

## Steps
1. Make initial request to populate cache
   - **Target**: `GET /api/contacts/alice@example.com/topics`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK, note the `topics` array and `cached` value

2. Immediately make a second request
   - **Target**: `GET /api/contacts/alice@example.com/topics`
   - **Input**: Header `X-Session-Token: {token}`
   - **Expected**: 200 OK with `cached: true`

3. Verify cached response matches original
   - **Target**: Compare response bodies
   - **Input**: Diff step 1 and step 2 responses
   - **Expected**: `topics` array is identical, `total_emails` is identical, only `cached` field differs (now `true`)

## Success Criteria
- [ ] First request returns `cached: false` (or `true` if already cached)
- [ ] Second request returns `cached: true`
- [ ] Topics array is identical between both responses
- [ ] `total_emails` is identical between both responses
- [ ] Second request completes faster than first (no AI call)

## Failure Criteria
- Second request returns `cached: false` (cache not working)
- Topics differ between requests within TTL window
- Second request triggers AI generation (latency similar to first)

## Notes
Cache TTL is 3600 seconds (1 hour). The second request within this window should return the cached result without invoking the AI provider. To test cache expiry, you would need to wait >1 hour or manually clear the `contact_topics_cache` table.
