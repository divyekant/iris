---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
---

# Trust Indicators

Iris evaluates the authenticity of incoming emails and detects tracking pixels, giving you visibility into message security and privacy.

## Email Authentication (SPF/DKIM/DMARC)

When you view a message or thread, Iris parses the `Authentication-Results` header from your mail server and displays trust badges for three email authentication protocols:

| Protocol | What it verifies |
|---|---|
| **SPF** (Sender Policy Framework) | The sending server is authorized to send on behalf of the sender's domain |
| **DKIM** (DomainKeys Identified Mail) | The email has not been tampered with in transit (cryptographic signature) |
| **DMARC** (Domain-based Message Authentication) | The domain owner's policy for handling authentication failures |

### Trust Badges

Each protocol shows one of these statuses:

| Status | Meaning | Badge Color |
|---|---|---|
| **Pass** | Authentication succeeded | Green |
| **Fail** | Authentication failed -- be cautious | Red |
| **Softfail** | Weak failure -- sender may be legitimate | Yellow |
| **Neutral** | No definitive result | Gray |
| **None** | No authentication record found | Gray |

Trust badges appear next to each message in the thread view. A message with all three protocols passing (green) is highly trustworthy. A message with one or more failures deserves extra scrutiny.

### No Configuration Needed

Trust indicators work automatically. Iris reads the authentication results that your mail server already includes in message headers. There is nothing to configure.

## Tracking Pixel Detection

Many marketing emails contain invisible "tracking pixels" -- tiny (1x1 pixel) images that notify the sender when you open the email. Iris detects these and alerts you.

### How It Works

When you open a message, Iris scans the HTML body for:

1. **Tiny images** -- `<img>` tags with width and height of 1 pixel or less
2. **Known tracker domains** -- image sources from well-known email tracking services

Detected trackers include services from providers like Mailchimp, SendGrid, HubSpot, Mailtrack, and others.

### What You See

If tracking pixels are detected, you see a **tracking alert** in the message view indicating:

- The number of tracking pixels found
- The domains that would be notified when you open the email

### Privacy Note

Iris blocks external image loading by default through its sandboxed iframe rendering. Tracking pixels are detected by analyzing the HTML source, not by loading the images. This means the tracking services do **not** receive a notification just because you opened the email in Iris.
