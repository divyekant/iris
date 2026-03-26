---
id: feat-023
type: feature-doc
audience: external
topic: showcase-features
status: current
generated: 2026-03-26
hermes-version: 1.0.1
---

# Smart Email Intelligence

## Overview

Iris includes six intelligence and automation features that work together to make your inbox more aware, more proactive, and less manual. These features learn from your email history, recognize patterns, and take action on your behalf -- so you spend less time managing email and more time on what matters.

- **Knowledge Graph** -- automatically maps everyone and everything mentioned in your emails
- **Temporal Reasoning** -- search by time ("emails from last week") and see a timeline of upcoming events
- **Writing Style Learning** -- AI drafts now sound like you
- **Auto-Draft** -- routine emails get pre-written replies waiting for your approval
- **Email Delegation** -- configure playbooks to handle specific email types automatically, with undo support and action history
- **Evolving Categories** -- your inbox organizes itself as your email patterns change, with category explanations and accept/dismiss workflow

---

## Knowledge Graph

### What it does

As Iris processes your emails, it builds a knowledge graph of people, organizations, projects, and topics it encounters. You can search this graph to find everything related to a person or project -- across all threads, all time.

### How to use it

**Search the graph:**

```
GET /api/graph?query=Sarah
```

Returns all entities named "Sarah" with their relations and the thread IDs where she appears. Use this to quickly find all emails connected to a person or topic.

**List all entities:**

```
GET /api/graph/entities
```

Returns the full list of entities in the knowledge graph with their types and relationship counts.

**Extract entities from a specific message:**

```
POST /api/graph/extract/{message_id}
```

Triggers on-demand extraction for a specific message if it hasn't been processed yet.

**Example response:**

```json
{
  "entities": [
    {
      "id": 42,
      "name": "Sarah Chen",
      "type": "person",
      "thread_ids": ["thread-abc", "thread-def"],
      "relations": [
        { "entity": "Acme Corp", "type": "affiliated_with" }
      ]
    }
  ]
}
```

---

## Temporal Reasoning

### What it does

You can now search your email using natural time references. Iris resolves phrases like "emails from last week" or "messages around the product launch" into real date ranges and returns matching emails.

Iris also maintains a timeline of calendar events it finds in emails -- meetings, deadlines, and reminders -- in one place.

### How to use it

**Search by time reference:**

```
POST /api/search/temporal
Content-Type: application/json

{ "query": "emails from last week" }
```

**Response:**

```json
{
  "resolved_range": {
    "from": "2026-03-08T00:00:00Z",
    "to": "2026-03-14T23:59:59Z"
  },
  "messages": [ ... ]
}
```

**View your email timeline:**

```
GET /api/timeline
```

Returns all extracted events (meetings, deadlines, reminders) sorted by date ascending. Useful for building a calendar view or getting a quick overview of upcoming commitments.

---

## Writing Style Learning

### What it does

Iris analyzes your sent emails to learn how you write -- your typical greeting, your sign-off, and your overall tone (formal, casual, or neutral). All AI-generated draft content then matches your style automatically.

### How to use it

1. Go to **Settings > AI > Writing Style**
2. Click **Analyze my writing style**
3. Iris scans your sent mail and displays your detected profile

Your profile shows:
- **Greeting**: the opening phrase you use most (e.g., "Hi", "Hello", "Hey")
- **Sign-off**: your closing phrase (e.g., "Best", "Thanks", "Cheers")
- **Tone**: formal, casual, or neutral

Once saved, every AI-assisted draft -- whether from Auto-Draft, AI Assist in Compose, or delegation auto-replies -- will match your detected style.

**Re-analyze at any time** to update the profile as your writing evolves.

**API:**

```
POST /api/style/analyze/{account_id}   -- run analysis and save profile
GET  /api/style/{account_id}           -- fetch stored profile
```

---

## Auto-Draft

### What it does

Iris recognizes recurring email patterns from your history (invoice confirmations, meeting acknowledgements, status update requests) and pre-generates draft replies for new messages that match those patterns. A **"Draft ready"** chip appears in the thread view so you can review and send with one click.

### How to use it

When you see a **"Draft ready"** chip on a thread, click it to open the pre-written reply in Compose. Review the draft, make any edits, and send -- or dismiss it if it's not needed.

**Tell Iris when a draft was helpful:**

Iris improves over time based on your feedback. When you use a pre-generated draft, its confidence score increases. When you dismiss it, the pattern is down-weighted.

**Configure auto-draft behavior:**

```
GET  /api/config/auto-draft            -- view current settings
PUT  /api/config/auto-draft            -- update settings
```

**API:**

```
GET  /api/auto-draft/{message_id}                   -- check if a draft is available
POST /api/auto-draft/generate/{message_id}          -- generate a draft on demand
POST /api/auto-draft/{draft_id}/feedback            -- submit feedback
```

**Feedback payload:**

```json
{ "action": "used" }      // increases pattern confidence
{ "action": "dismissed" } // decreases pattern confidence
```

---

## Email Delegation

### What it does

Delegation lets you define playbooks -- sets of trigger conditions and automated actions -- so Iris handles specific types of email without your involvement. Each incoming email is evaluated against your active playbooks. Actions are logged and can be undone.

### How to use it

Go to **Settings > Delegation** to create and manage playbooks.

**Supported triggers** (any combination):
- Subject contains / equals / starts with a phrase
- Sender matches an email address
- Category equals (Primary, Updates, Social, Promotions, or a custom category)
- Body contains a keyword

**Supported actions:**

| Action | What happens |
|---|---|
| Auto-reply | Sends a reply using your template text |
| Draft reply | Creates a draft using your template (you send it) |
| Forward | Forwards the email to another address |
| Archive | Moves the email to Archive |
| Label | Applies a label to the email |

**Example playbook -- auto-acknowledge invoices:**

```json
{
  "name": "Auto-reply to invoice confirmations",
  "trigger": {
    "conditions": [
      { "field": "subject", "operator": "contains", "value": "invoice" }
    ],
    "match": "all"
  },
  "action": {
    "type": "auto_reply",
    "template": "Thank you -- received. We'll process this within 5 business days."
  }
}
```

**View action history and undo:**

You can review all actions Delegation has taken and undo recent ones:

```
GET  /api/delegation/actions              -- list all executed actions
POST /api/delegation/actions/{id}/undo    -- undo a specific action
GET  /api/delegation/summary              -- overview of delegation activity
```

**API:**

```
POST   /api/delegation/playbooks              -- create playbook
GET    /api/delegation/playbooks              -- list all playbooks
PUT    /api/delegation/playbooks/{id}         -- update / enable / disable
DELETE /api/delegation/playbooks/{id}         -- delete
POST   /api/delegation/process/{message_id}   -- manually trigger evaluation
```

---

## Evolving Categories

### What it does

Standard inbox tabs (Primary, Updates, Social, Promotions) only go so far. Evolving Categories watches your email patterns and suggests new category tabs tailored to your inbox -- like "Dev Notifications", "Finance", or "HR". You decide which suggestions to accept, and the new tabs appear right in your inbox.

### How to use it

1. Go to **Settings > Categories**
2. Click **Analyze my email patterns**
3. Review AI-suggested categories with their message counts and confidence scores
4. Accept suggestions to add them as inbox tabs -- or dismiss ones you don't need

Accepted categories appear as tabs in your inbox alongside the standard four. Clicking a custom tab shows only messages in that category.

**Accept or dismiss suggestions:**

```
POST /api/categories/custom/{id}/accept    -- accept a suggestion as a new tab
POST /api/categories/custom/{id}/dismiss   -- dismiss a suggestion
```

**Understand why a message is in a category:**

```
GET /api/categories/explain/{message_id}   -- see the AI's reasoning
```

**Remove a custom category** at any time from Settings > Categories -- this removes the tab but does not delete the underlying messages.

**API:**

```
POST   /api/categories/analyze/{account_id}   -- generate suggestions
GET    /api/categories/custom                  -- list custom categories
POST   /api/categories/custom                  -- create custom category
PUT    /api/categories/custom/{id}             -- update custom category
DELETE /api/categories/custom/{id}             -- remove custom category
```

---

## Tips

- **Writing Style + Auto-Draft** work best together: once your style profile is set, auto-draft replies will already sound like you.
- **Knowledge Graph** builds incrementally -- the more emails Iris processes, the richer the graph becomes.
- **Delegation playbooks** are evaluated in creation order. Put your most specific rules first to avoid false matches.
- **Category analysis** requires at least 20 messages. Run it after a few days of normal inbox use for the best suggestions.
- **Delegation actions can be undone** from the action history. Review the summary page to see what Iris has been doing on your behalf.
- **Category explanations** help you understand why Iris categorized a message the way it did, making it easier to refine your categories over time.
