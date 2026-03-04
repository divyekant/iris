---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
---

# Email Management

Iris lets you connect multiple email accounts, browse your unified inbox, read threads, and manage messages with batch actions.

## Connecting Email Accounts

You can connect accounts from three providers. Each account syncs independently and stays up to date via IMAP IDLE (real-time push notifications).

### Gmail (OAuth)

1. Go to **Settings > Accounts** or click **Add Account** on the setup page.
2. Select **Gmail**.
3. You will be redirected to Google to authorize Iris. Grant the requested permissions.
4. After authorization, you are redirected back to Iris and your account begins syncing.

**Prerequisite:** You need to configure `GMAIL_CLIENT_ID` and `GMAIL_CLIENT_SECRET` in your `.env` file. These come from a Google Cloud Console project with the Gmail API enabled. Set the OAuth redirect URI to `http://localhost:3000/auth/callback`.

### Outlook (OAuth)

1. Go to **Settings > Accounts** or click **Add Account** on the setup page.
2. Select **Outlook**.
3. You will be redirected to Microsoft to authorize Iris.
4. After authorization, your account begins syncing.

**Prerequisite:** You need to configure `OUTLOOK_CLIENT_ID` and `OUTLOOK_CLIENT_SECRET` in your `.env` file. These come from an Azure AD app registration. Set the OAuth redirect URI to `http://localhost:3000/auth/callback`.

### IMAP (Manual)

For any email provider that supports IMAP:

1. Go to **Settings > Accounts** or click **Add Account**.
2. Select **IMAP**.
3. Enter your IMAP server hostname, port, username, and password.
4. Iris tests the connection and begins syncing if successful.

## Unified Inbox

When you have multiple accounts connected, the inbox shows messages from all active accounts merged together, sorted by date (newest first). You can filter to a single account using the **account switcher** in the sidebar.

## Category Tabs

Iris organizes your inbox with category tabs at the top:

| Tab | Description |
|---|---|
| **Primary** | Important personal and work emails |
| **Updates** | Notifications, confirmations, and status updates |
| **Social** | Messages from social networks and communities |
| **Promotions** | Marketing emails and offers |

Categories are assigned by the AI classification system when enabled. Without AI, all messages appear under the default view.

## Reading Emails

Click any message in the inbox to open the **thread view**. The thread view shows:

- All messages in the conversation, in chronological order
- Sender information and timestamps for each message
- Full HTML or plain-text email body (rendered securely in a sandboxed iframe)
- Trust indicators (SPF/DKIM/DMARC badges) per message
- Tracking pixel detection alerts
- AI thread summary (if AI is enabled, shown in a collapsible panel)

## Message Actions

From the thread view, you can take actions on individual messages:

- **Star** / **Unstar** -- flag important messages
- **Archive** -- move to the Archive folder
- **Delete** -- move to the Trash folder
- **Mark Unread** -- mark the message as unread
- **Reply** / **Reply All** / **Forward** -- open the compose modal

## Batch Actions

In the inbox, you can select multiple messages using the checkboxes on the left side, then apply an action to all of them at once:

- **Archive** -- move selected messages to Archive
- **Delete** -- move selected messages to Trash
- **Mark Read** -- mark all as read
- **Mark Unread** -- mark all as unread
- **Star** -- star all selected messages
- **Unstar** -- remove stars from selected messages

## View Modes

Iris supports two view modes, configurable in **Settings**:

- **Traditional** -- classic email layout with a message list and separate thread view
- **Messaging** -- a more conversational, chat-like layout for threads

Switch between them in **Settings > View Mode** or by using the toggle in the inbox toolbar.

## Composing and Sending Email

Click the **Compose** button to open the compose modal. You can:

- Write a new email from scratch
- Reply, reply all, or forward from within a thread
- Save drafts (auto-saved periodically)
- Use AI writing assist to refine your text (see [AI Writing Assist](feat-004-ai-writing-assist.md))

Sending uses SMTP with XOAUTH2 authentication for Gmail and Outlook accounts. Sent messages are stored locally in the Sent folder.

## Sync and Real-Time Updates

Iris keeps your inbox current in two ways:

- **Initial sync** -- downloads existing messages from your IMAP server when an account is first connected.
- **IMAP IDLE** -- maintains a persistent connection to your mail server for real-time push notifications of new messages. The connection refreshes every 29 minutes per the IMAP specification.

New messages trigger a WebSocket notification to the frontend, so your inbox updates without manual refresh.
