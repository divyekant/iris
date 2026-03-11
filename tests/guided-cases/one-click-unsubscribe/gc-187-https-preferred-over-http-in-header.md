# GC-187: Multiple URLs in Header — HTTPS Preferred Over HTTP

## Metadata
- **Type**: edge
- **Priority**: P1
- **Surface**: api
- **Flow**: one-click-unsubscribe
- **Tags**: unsubscribe, url-preference, https, header-parsing, edge
- **Generated**: 2026-03-10
- **Last Executed**: never

## Preconditions
### Environment
- App running at http://127.0.0.1:3000

### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)
- A synced message exists whose `List-Unsubscribe` header contains multiple values — one HTTPS URL and one HTTP URL — e.g., `<https://secure.example.com/unsub>, <http://legacy.example.com/unsub>`
- Note the message ID and both URLs

## Steps
1. Fetch the message detail
   - **Target**: GET /api/messages/{id}
   - **Expected**: 200 OK with `list_unsubscribe` equal to `https://secure.example.com/unsub` (the HTTPS URL), not the HTTP one

## Success Criteria
- [ ] Response status is 200
- [ ] `list_unsubscribe` contains the HTTPS URL specifically
- [ ] The HTTP-only URL is not stored (HTTPS takes priority per parse_list_unsubscribe logic)

## Failure Criteria
- `list_unsubscribe` contains the HTTP URL when an HTTPS URL was also present
- `list_unsubscribe` contains a comma-separated list of both URLs (raw header stored unparsed)
- Server error (500)

## Notes
- URL priority order in `parse_list_unsubscribe`: HTTPS > HTTP > mailto
- If only HTTP and mailto are present, HTTP should be preferred over mailto
