use rusqlite::Connection;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Tool definitions
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Result returned by a tool handler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub message: String,
    /// Optional structured data for frontend rendering (e.g. compose card).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// All tools available to the chat agent.
pub fn all_tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "compose_email".to_string(),
            description: "Draft a new email or reply based on the user's instructions. Use this when the user asks to write, draft, compose, or reply to an email via chat.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "to": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "Recipient email addresses"
                    },
                    "cc": {
                        "type": "array",
                        "items": { "type": "string" },
                        "description": "CC recipients (optional)"
                    },
                    "subject": {
                        "type": "string",
                        "description": "Email subject line"
                    },
                    "body": {
                        "type": "string",
                        "description": "Email body in HTML format"
                    },
                    "reply_to_message_id": {
                        "type": "string",
                        "description": "If replying, the message_id to reply to (optional)"
                    },
                    "tone": {
                        "type": "string",
                        "enum": ["formal", "casual", "brief"],
                        "description": "Writing tone (optional, defaults to formal)"
                    }
                },
                "required": ["to", "subject", "body"]
            }),
        },
    ]
}

// ---------------------------------------------------------------------------
// Compose email types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposeEmailArgs {
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    pub subject: String,
    pub body: String,
    pub reply_to_message_id: Option<String>,
    pub tone: Option<String>,
}

/// Data stored in ProposedAction for compose_email confirmations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComposeEmailData {
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: String,
    pub body: String,
    pub reply_to_message_id: Option<String>,
    pub tone: Option<String>,
    /// Thread context fetched at compose time (if replying).
    pub thread_subject: Option<String>,
}

// ---------------------------------------------------------------------------
// Tool handler
// ---------------------------------------------------------------------------

/// Handle a compose_email tool call.
///
/// Validates the arguments and returns a summary message. The actual draft is
/// NOT created here — it is deferred to the confirm_action flow so the user
/// can review first.
pub fn handle_compose_email(conn: &Connection, tool_call: &ToolCall) -> ToolResult {
    // Parse arguments
    let args: ComposeEmailArgs = match serde_json::from_value(tool_call.arguments.clone()) {
        Ok(a) => a,
        Err(e) => {
            return ToolResult {
                success: false,
                message: format!("Invalid compose_email arguments: {e}"),
                data: None,
            };
        }
    };

    // Validate: at least one recipient
    if args.to.is_empty() {
        return ToolResult {
            success: false,
            message: "At least one recipient is required in the 'to' field.".to_string(),
            data: None,
        };
    }

    // Validate: non-empty subject
    if args.subject.trim().is_empty() {
        return ToolResult {
            success: false,
            message: "Subject cannot be empty.".to_string(),
            data: None,
        };
    }

    // Validate: non-empty body
    if args.body.trim().is_empty() {
        return ToolResult {
            success: false,
            message: "Email body cannot be empty.".to_string(),
            data: None,
        };
    }

    // Validate email addresses (basic check)
    for addr in &args.to {
        if !addr.contains('@') || addr.len() < 3 {
            return ToolResult {
                success: false,
                message: format!("Invalid email address: {addr}"),
                data: None,
            };
        }
    }
    for addr in &args.cc {
        if !addr.contains('@') || addr.len() < 3 {
            return ToolResult {
                success: false,
                message: format!("Invalid CC email address: {addr}"),
                data: None,
            };
        }
    }

    // If replying, look up the original message for thread context
    let thread_subject = if let Some(ref reply_id) = args.reply_to_message_id {
        conn.query_row(
            "SELECT subject FROM messages WHERE id = ?1",
            rusqlite::params![reply_id],
            |row| row.get::<_, Option<String>>(0),
        )
        .ok()
        .flatten()
    } else {
        None
    };

    // Build the compose data that will be stored in the proposed_action
    let compose_data = ComposeEmailData {
        to: args.to.clone(),
        cc: args.cc.clone(),
        subject: args.subject.clone(),
        body: args.body.clone(),
        reply_to_message_id: args.reply_to_message_id.clone(),
        tone: args.tone.clone(),
        thread_subject,
    };

    // Build a human-readable summary
    let recipients = args.to.join(", ");
    let tone_label = args.tone.as_deref().unwrap_or("formal");

    let mut summary = format!(
        "Drafted email to {recipients} with subject '{}' (tone: {tone_label}).",
        args.subject
    );
    if args.reply_to_message_id.is_some() {
        summary.push_str(" This is a reply to an existing thread.");
    }
    summary.push_str(" Review and send from the compose panel.");

    ToolResult {
        success: true,
        message: summary,
        data: Some(serde_json::to_value(&compose_data).unwrap_or_default()),
    }
}

/// Execute a confirmed compose_email action: save the draft to the database.
///
/// Returns the draft ID on success. Accepts any type that derefs to
/// `rusqlite::Connection` (both `PooledConnection` and plain `Connection`).
pub fn execute_compose_email(
    conn: &Connection,
    compose_data: &ComposeEmailData,
    account_id: &str,
) -> Result<String, String> {
    let to_json = serde_json::to_string(&compose_data.to).ok();
    let cc_json = if compose_data.cc.is_empty() {
        None
    } else {
        serde_json::to_string(&compose_data.cc).ok()
    };

    // Strip HTML tags for body_text (plain-text version)
    let body_text = strip_html_tags(&compose_data.body);
    let snippet: String = body_text.chars().take(200).collect();

    let id = uuid::Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO messages (
            id, account_id, folder, to_addresses, cc_addresses, bcc_addresses,
            subject, body_text, body_html, snippet,
            is_read, is_starred, is_draft, date, from_address, from_name, has_attachments
        ) VALUES (
            ?1, ?2, 'Drafts', ?3, ?4, NULL,
            ?5, ?6, ?7, ?8,
            1, 0, 1, unixepoch(), NULL, NULL, 0
        )",
        rusqlite::params![
            id,
            account_id,
            to_json,
            cc_json,
            compose_data.subject,
            body_text,
            compose_data.body,
            snippet,
        ],
    )
    .map_err(|e| format!("Failed to save draft: {e}"))?;

    Ok(id)
}

/// Simple HTML tag stripper for generating plain text from HTML body.
fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE messages (
                id TEXT PRIMARY KEY,
                account_id TEXT NOT NULL DEFAULT '',
                folder TEXT NOT NULL DEFAULT 'INBOX',
                from_address TEXT,
                from_name TEXT,
                to_addresses TEXT,
                cc_addresses TEXT,
                bcc_addresses TEXT,
                subject TEXT,
                snippet TEXT,
                body_text TEXT,
                body_html TEXT,
                date INTEGER,
                is_read INTEGER NOT NULL DEFAULT 0,
                is_starred INTEGER NOT NULL DEFAULT 0,
                is_draft INTEGER NOT NULL DEFAULT 0,
                has_attachments INTEGER NOT NULL DEFAULT 0,
                labels TEXT,
                ai_priority_label TEXT,
                ai_category TEXT,
                thread_id TEXT,
                message_id TEXT,
                uid INTEGER,
                modseq INTEGER,
                raw_headers TEXT,
                attachment_names TEXT,
                size_bytes INTEGER,
                updated_at INTEGER DEFAULT (unixepoch()),
                created_at INTEGER DEFAULT (unixepoch())
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_compose_email_in_all_tools() {
        let tools = all_tools();
        assert!(
            tools.iter().any(|t| t.name == "compose_email"),
            "compose_email should be in all_tools()"
        );
    }

    #[test]
    fn test_compose_email_tool_definition() {
        let tools = all_tools();
        let compose = tools.iter().find(|t| t.name == "compose_email").unwrap();
        let required = compose.parameters["required"].as_array().unwrap();
        let required_names: Vec<&str> = required.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(required_names.contains(&"to"));
        assert!(required_names.contains(&"subject"));
        assert!(required_names.contains(&"body"));
    }

    #[test]
    fn test_handle_compose_all_fields() {
        let conn = make_test_db();
        let tool_call = ToolCall {
            name: "compose_email".to_string(),
            arguments: serde_json::json!({
                "to": ["alice@example.com"],
                "cc": ["bob@example.com"],
                "subject": "Project Update",
                "body": "<p>Here is the update.</p>",
                "tone": "formal"
            }),
        };
        let result = handle_compose_email(&conn, &tool_call);
        assert!(result.success);
        assert!(result.message.contains("alice@example.com"));
        assert!(result.message.contains("Project Update"));
        assert!(result.message.contains("formal"));
        assert!(result.data.is_some());

        let data: ComposeEmailData =
            serde_json::from_value(result.data.unwrap()).unwrap();
        assert_eq!(data.to, vec!["alice@example.com"]);
        assert_eq!(data.cc, vec!["bob@example.com"]);
        assert_eq!(data.subject, "Project Update");
    }

    #[test]
    fn test_handle_compose_minimal_fields() {
        let conn = make_test_db();
        let tool_call = ToolCall {
            name: "compose_email".to_string(),
            arguments: serde_json::json!({
                "to": ["alice@example.com"],
                "subject": "Hello",
                "body": "Hi there"
            }),
        };
        let result = handle_compose_email(&conn, &tool_call);
        assert!(result.success);
        assert!(result.message.contains("alice@example.com"));
        assert!(result.message.contains("Hello"));
        // Default tone should be formal
        assert!(result.message.contains("formal"));

        let data: ComposeEmailData =
            serde_json::from_value(result.data.unwrap()).unwrap();
        assert!(data.cc.is_empty());
        assert!(data.tone.is_none());
        assert!(data.reply_to_message_id.is_none());
    }

    #[test]
    fn test_handle_compose_reply() {
        let conn = make_test_db();
        // Insert a message to reply to
        conn.execute(
            "INSERT INTO messages (id, account_id, subject, folder) VALUES ('msg-123', 'acc-1', 'Original Subject', 'INBOX')",
            [],
        )
        .unwrap();

        let tool_call = ToolCall {
            name: "compose_email".to_string(),
            arguments: serde_json::json!({
                "to": ["sender@example.com"],
                "subject": "Re: Original Subject",
                "body": "Thanks for your email.",
                "reply_to_message_id": "msg-123"
            }),
        };
        let result = handle_compose_email(&conn, &tool_call);
        assert!(result.success);
        assert!(result.message.contains("reply"));

        let data: ComposeEmailData =
            serde_json::from_value(result.data.unwrap()).unwrap();
        assert_eq!(data.reply_to_message_id.as_deref(), Some("msg-123"));
        assert_eq!(data.thread_subject.as_deref(), Some("Original Subject"));
    }

    #[test]
    fn test_handle_compose_reply_nonexistent_message() {
        let conn = make_test_db();
        let tool_call = ToolCall {
            name: "compose_email".to_string(),
            arguments: serde_json::json!({
                "to": ["sender@example.com"],
                "subject": "Re: Something",
                "body": "Reply text",
                "reply_to_message_id": "nonexistent-id"
            }),
        };
        let result = handle_compose_email(&conn, &tool_call);
        // Should still succeed — just no thread context
        assert!(result.success);
        let data: ComposeEmailData =
            serde_json::from_value(result.data.unwrap()).unwrap();
        assert!(data.thread_subject.is_none());
    }

    #[test]
    fn test_handle_compose_missing_to() {
        let conn = make_test_db();
        let tool_call = ToolCall {
            name: "compose_email".to_string(),
            arguments: serde_json::json!({
                "to": [],
                "subject": "Hello",
                "body": "Content"
            }),
        };
        let result = handle_compose_email(&conn, &tool_call);
        assert!(!result.success);
        assert!(result.message.contains("recipient"));
    }

    #[test]
    fn test_handle_compose_missing_subject() {
        let conn = make_test_db();
        let tool_call = ToolCall {
            name: "compose_email".to_string(),
            arguments: serde_json::json!({
                "to": ["a@b.com"],
                "subject": "",
                "body": "Content"
            }),
        };
        let result = handle_compose_email(&conn, &tool_call);
        assert!(!result.success);
        assert!(result.message.contains("Subject"));
    }

    #[test]
    fn test_handle_compose_missing_body() {
        let conn = make_test_db();
        let tool_call = ToolCall {
            name: "compose_email".to_string(),
            arguments: serde_json::json!({
                "to": ["a@b.com"],
                "subject": "Hello",
                "body": "   "
            }),
        };
        let result = handle_compose_email(&conn, &tool_call);
        assert!(!result.success);
        assert!(result.message.contains("body"));
    }

    #[test]
    fn test_handle_compose_invalid_email() {
        let conn = make_test_db();
        let tool_call = ToolCall {
            name: "compose_email".to_string(),
            arguments: serde_json::json!({
                "to": ["not-an-email"],
                "subject": "Hello",
                "body": "Content"
            }),
        };
        let result = handle_compose_email(&conn, &tool_call);
        assert!(!result.success);
        assert!(result.message.contains("Invalid email"));
    }

    #[test]
    fn test_handle_compose_invalid_cc_email() {
        let conn = make_test_db();
        let tool_call = ToolCall {
            name: "compose_email".to_string(),
            arguments: serde_json::json!({
                "to": ["valid@example.com"],
                "cc": ["bad"],
                "subject": "Hello",
                "body": "Content"
            }),
        };
        let result = handle_compose_email(&conn, &tool_call);
        assert!(!result.success);
        assert!(result.message.contains("Invalid CC"));
    }

    #[test]
    fn test_handle_compose_tone_handling() {
        let conn = make_test_db();

        for tone in &["formal", "casual", "brief"] {
            let tool_call = ToolCall {
                name: "compose_email".to_string(),
                arguments: serde_json::json!({
                    "to": ["a@b.com"],
                    "subject": "Test",
                    "body": "Content",
                    "tone": tone
                }),
            };
            let result = handle_compose_email(&conn, &tool_call);
            assert!(result.success, "Should succeed with tone {tone}");
            assert!(
                result.message.contains(tone),
                "Summary should mention tone {tone}"
            );
            let data: ComposeEmailData =
                serde_json::from_value(result.data.unwrap()).unwrap();
            assert_eq!(data.tone.as_deref(), Some(*tone));
        }
    }

    #[test]
    fn test_handle_compose_invalid_arguments() {
        let conn = make_test_db();
        let tool_call = ToolCall {
            name: "compose_email".to_string(),
            arguments: serde_json::json!({
                "to": "not-an-array",
                "subject": "Hello"
            }),
        };
        let result = handle_compose_email(&conn, &tool_call);
        assert!(!result.success);
        assert!(result.message.contains("Invalid"));
    }

    #[test]
    fn test_strip_html_tags() {
        assert_eq!(strip_html_tags("<p>Hello <b>world</b></p>"), "Hello world");
        assert_eq!(strip_html_tags("no tags"), "no tags");
        assert_eq!(strip_html_tags("<br/>line<br/>break"), "linebreak");
        assert_eq!(strip_html_tags(""), "");
    }

    #[test]
    fn test_execute_compose_email() {
        let conn = make_test_db();
        let data = ComposeEmailData {
            to: vec!["alice@example.com".to_string()],
            cc: vec![],
            subject: "Test Draft".to_string(),
            body: "<p>Draft body</p>".to_string(),
            reply_to_message_id: None,
            tone: Some("casual".to_string()),
            thread_subject: None,
        };
        let result = execute_compose_email(&conn, &data, "acc-1");
        assert!(result.is_ok());
        let draft_id = result.unwrap();
        assert!(!draft_id.is_empty());

        // Verify draft was saved
        let is_draft: bool = conn
            .query_row(
                "SELECT is_draft FROM messages WHERE id = ?1",
                rusqlite::params![draft_id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(is_draft);
    }

    #[test]
    fn test_execute_compose_email_with_cc() {
        let conn = make_test_db();
        let data = ComposeEmailData {
            to: vec!["alice@example.com".to_string()],
            cc: vec!["bob@example.com".to_string()],
            subject: "CC Test".to_string(),
            body: "Body".to_string(),
            reply_to_message_id: None,
            tone: None,
            thread_subject: None,
        };
        let result = execute_compose_email(&conn, &data, "acc-1");
        assert!(result.is_ok());

        let draft_id = result.unwrap();
        let cc_json: Option<String> = conn
            .query_row(
                "SELECT cc_addresses FROM messages WHERE id = ?1",
                rusqlite::params![draft_id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(cc_json.is_some());
        assert!(cc_json.unwrap().contains("bob@example.com"));
    }
}
