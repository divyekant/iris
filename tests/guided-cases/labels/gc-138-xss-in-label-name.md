# GC-138: XSS Payload in Label Name

## Metadata
- **Type**: security
- **Priority**: P0
- **Surface**: api
- **Flow**: labels
- **Tags**: labels, security, xss
- **Generated**: 2026-03-09
- **Last Executed**: 2026-03-09

## Preconditions
### Environment
- Iris running at http://127.0.0.1:3000
### Data
- Session token obtained (source: inline, setup: GET /api/auth/bootstrap)

## Steps
1. Create a label with an XSS payload as the name
   - **Target**: POST /api/labels
   - **Input**: `{"name": "<script>alert('xss')</script>", "color": "#FF0000"}`
   - **Expected**: Either 400 (rejected) or 201 with the XSS string stored as-is (not executed, safe if output-encoded on render)
2. If created, verify the stored value via GET /api/labels
   - **Target**: GET /api/labels
   - **Input**: none
   - **Expected**: The label name is returned as a plain string, not interpreted as HTML

## Success Criteria
- [ ] Server does not crash (no 500)
- [ ] If 201: name is stored as literal string `<script>alert('xss')</script>` (no execution)
- [ ] If 400: XSS payload is rejected outright
- [ ] GET response Content-Type is application/json (not text/html)

## Failure Criteria
- Server error (500)
- Response Content-Type is text/html (could render XSS)
- Name is silently modified or truncated without indication
