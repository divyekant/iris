---
status: draft
generated: 2026-03-04
source-tier: direct
hermes-version: 1.0.0
---

# AI Feedback

Iris lets you correct AI classifications to improve future accuracy. When the AI assigns the wrong category, priority, or intent to an email, you can fix it -- and the system learns from your corrections over time.

## How to Submit a Correction

1. Open a message in the thread view.
2. Look at the AI classification labels (category, priority, intent).
3. Click on the classification you want to correct.
4. Select the correct value from the available options.
5. The correction is saved immediately.

## Correctable Fields

You can correct three classification fields:

### Category

The inbox category assigned to the message.

| Valid Values |
|---|
| Primary |
| Updates |
| Social |
| Promotions |
| Finance |
| Travel |
| Newsletters |

### Priority

The urgency level assigned to the message.

| Valid Values |
|---|
| urgent |
| high |
| normal |
| low |

### Intent

The type of email as classified by the AI.

| Valid Values |
|---|
| ACTION_REQUEST |
| INFORMATIONAL |
| TRANSACTIONAL |
| SOCIAL |
| MARKETING |
| NOTIFICATION |

## How It Improves Accuracy

Iris stores every correction you make in a feedback log. When the AI classifies future emails, it receives a summary of your most common correction patterns as additional context. For example, if you repeatedly change emails from a certain type from "Promotions" to "Updates," the AI adjusts its behavior accordingly.

The feedback loop activates once a correction pattern occurs at least 2 times. The top 10 most frequent correction patterns are included in the AI's classification prompt.

Additionally, every 10 corrections trigger a preference extraction job. This job analyzes your correction history, identifies recurring patterns, and stores them as structured preferences in your Memories instance. These preferences persist across sessions and are included in future classification prompts, so the AI's accuracy improves steadily over time -- even after a server restart.

## Viewing Feedback Statistics

You can see a summary of all corrections you have made:

1. Go to **Settings > AI**.
2. Look for the **Feedback Statistics** section.

The statistics include:

- **Total corrections** -- how many times you have corrected the AI
- **Corrections by field** -- breakdown by category, priority, and intent
- **Common correction patterns** -- the most frequent "from X to Y" corrections, showing what the AI is getting wrong most often

## Feedback API

If you are integrating programmatically, the feedback endpoints are:

**Submit a correction:**

```
PUT /api/messages/{id}/ai-feedback
```

```json
{
  "field": "category",
  "value": "Primary"
}
```

**Get feedback statistics:**

```
GET /api/ai/feedback-stats
```

Response:

```json
{
  "total_corrections": 42,
  "by_field": [
    { "field": "category", "count": 25 },
    { "field": "priority_label", "count": 12 },
    { "field": "intent", "count": 5 }
  ],
  "common_corrections": [
    {
      "field": "category",
      "original": "Promotions",
      "corrected": "Updates",
      "count": 8
    }
  ]
}
```
