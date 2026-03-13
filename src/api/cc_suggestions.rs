use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

// ---------------------------------------------------------------------------
// POST /api/ai/suggest-cc
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SuggestCcRequest {
    pub thread_id: Option<String>,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub subject: String,
    pub body_preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CcSuggestion {
    pub email: String,
    pub name: Option<String>,
    pub reason: String,
    pub confidence: f64,
    #[serde(rename = "type")]
    pub suggestion_type: String, // "cc" or "bcc"
}

#[derive(Debug, Serialize)]
pub struct SuggestCcResponse {
    pub suggestions: Vec<CcSuggestion>,
}

/// Contact co-occurrence: someone who frequently appears in threads with the given recipients.
#[derive(Debug, Clone)]
pub struct CoOccurrence {
    pub email: String,
    pub name: Option<String>,
    pub count: i64,
}

/// Query contacts who frequently appear in threads alongside the given recipients.
/// Looks at both sender_email (from_address) and cc_addresses/to_addresses fields.
pub fn query_co_occurrences(
    conn: &r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>,
    recipients: &[String],
    limit: usize,
) -> Vec<CoOccurrence> {
    if recipients.is_empty() {
        return vec![];
    }

    // Build placeholders for the IN clause
    let placeholders: Vec<String> = (1..=recipients.len()).map(|i| format!("?{i}")).collect();
    let in_clause = placeholders.join(", ");

    // Query: find other senders who appear in threads that contain messages from the given recipients
    let sql = format!(
        "SELECT m2.from_address AS email, m2.from_name AS name, COUNT(*) AS co_count
         FROM messages m1
         JOIN messages m2 ON m1.thread_id = m2.thread_id
            AND m1.from_address != m2.from_address
         WHERE m1.from_address IN ({})
           AND m2.from_address IS NOT NULL
           AND m2.from_address NOT IN ({})
           AND m1.thread_id IS NOT NULL
           AND m1.is_deleted = 0
           AND m2.is_deleted = 0
         GROUP BY m2.from_address
         ORDER BY co_count DESC
         LIMIT ?{}",
        in_clause,
        in_clause,
        recipients.len() * 2 + 1
    );

    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    // First set: m1.from_address IN (?)
    for r in recipients {
        params.push(Box::new(r.clone()));
    }
    // Second set: m2.from_address NOT IN (?) — exclude current recipients
    for r in recipients {
        params.push(Box::new(r.clone()));
    }
    // LIMIT param
    params.push(Box::new(limit as i64));

    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("Co-occurrence query prepare error: {e}");
            return vec![];
        }
    };

    match stmt.query_map(param_refs.as_slice(), |row| {
        Ok(CoOccurrence {
            email: row.get("email")?,
            name: row.get("name")?,
            count: row.get("co_count")?,
        })
    }) {
        Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
        Err(e) => {
            tracing::warn!("Co-occurrence query error: {e}");
            vec![]
        }
    }
}

/// Get prior CC participants from a specific thread.
pub fn get_thread_cc_participants(
    conn: &r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>,
    thread_id: &str,
) -> Vec<String> {
    let mut stmt = match conn.prepare(
        "SELECT DISTINCT cc_addresses FROM messages
         WHERE thread_id = ?1 AND cc_addresses IS NOT NULL AND is_deleted = 0",
    ) {
        Ok(s) => s,
        Err(_) => return vec![],
    };

    let cc_jsons: Vec<String> = match stmt.query_map(rusqlite::params![thread_id], |row| {
        row.get::<_, String>(0)
    }) {
        Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
        Err(_) => return vec![],
    };

    let mut all_cc: Vec<String> = Vec::new();
    for json_str in &cc_jsons {
        if let Ok(addrs) = serde_json::from_str::<Vec<String>>(json_str) {
            for addr in addrs {
                let lower = addr.to_lowercase();
                if !all_cc.contains(&lower) {
                    all_cc.push(lower);
                }
            }
        }
    }
    all_cc
}

const CC_SUGGESTION_SYSTEM_PROMPT: &str = r#"You are an email assistant that suggests CC/BCC recipients. Given the email context and a list of contacts who frequently co-occur in threads with the current recipients, suggest who should be CC'd or BCC'd on this email.

Return a JSON array of suggestions. Each suggestion must have these fields:
- "email": the email address
- "name": display name if known, or null
- "reason": brief explanation of why they should be included (max 50 chars)
- "confidence": a number between 0 and 1 indicating how confident you are
- "type": either "cc" or "bcc"

Rules:
- Only suggest contacts from the provided candidate list
- Do not suggest anyone already in the To or CC fields
- Return at most 5 suggestions, ordered by confidence (highest first)
- If no suggestions are appropriate, return an empty array []
- Return ONLY the JSON array, no other text"#;

/// Build the user prompt for AI CC suggestion reasoning.
pub fn build_cc_prompt(
    subject: &str,
    body_preview: &str,
    to: &[String],
    cc: &[String],
    co_occurrences: &[CoOccurrence],
    thread_cc: &[String],
) -> String {
    let mut prompt = String::new();

    prompt.push_str(&format!("Subject: {}\n", subject));

    let preview = if body_preview.len() > 500 {
        format!("{}...", &body_preview[..500])
    } else {
        body_preview.to_string()
    };
    prompt.push_str(&format!("Body preview: {}\n\n", preview));

    prompt.push_str(&format!("Current To: {}\n", to.join(", ")));
    if !cc.is_empty() {
        prompt.push_str(&format!("Current CC: {}\n", cc.join(", ")));
    }

    if !thread_cc.is_empty() {
        prompt.push_str(&format!(
            "\nPrevious CC participants in this thread: {}\n",
            thread_cc.join(", ")
        ));
    }

    if co_occurrences.is_empty() {
        prompt.push_str("\nNo frequent co-correspondents found.\n");
    } else {
        prompt.push_str("\nFrequent co-correspondents (candidate list):\n");
        for co in co_occurrences {
            let name_part = co
                .name
                .as_deref()
                .map(|n| format!(" ({})", n))
                .unwrap_or_default();
            prompt.push_str(&format!(
                "- {}{} — appeared in {} shared threads\n",
                co.email, name_part, co.count
            ));
        }
    }

    prompt
}

/// Parse AI response into CcSuggestion list. Handles valid JSON, malformed JSON, and empty arrays.
pub fn parse_suggestions(ai_response: &str) -> Vec<CcSuggestion> {
    // Try to find a JSON array in the response (the AI might wrap it in markdown code blocks)
    let trimmed = ai_response.trim();

    // Try direct parse first
    if let Ok(suggestions) = serde_json::from_str::<Vec<CcSuggestion>>(trimmed) {
        return validate_suggestions(suggestions);
    }

    // Try to extract JSON array from markdown code blocks or surrounding text
    if let Some(start) = trimmed.find('[') {
        if let Some(end) = trimmed.rfind(']') {
            let json_slice = &trimmed[start..=end];
            if let Ok(suggestions) = serde_json::from_str::<Vec<CcSuggestion>>(json_slice) {
                return validate_suggestions(suggestions);
            }
        }
    }

    vec![]
}

/// Validate and clamp suggestion fields.
fn validate_suggestions(mut suggestions: Vec<CcSuggestion>) -> Vec<CcSuggestion> {
    for s in &mut suggestions {
        // Clamp confidence to [0, 1]
        s.confidence = s.confidence.clamp(0.0, 1.0);
        // Normalize type to cc or bcc
        if s.suggestion_type != "cc" && s.suggestion_type != "bcc" {
            s.suggestion_type = "cc".to_string();
        }
    }
    // Sort by confidence descending, take top 5
    suggestions.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal));
    suggestions.truncate(5);
    suggestions
}

/// Handler: POST /api/ai/suggest-cc
pub async fn suggest_cc(
    State(state): State<Arc<AppState>>,
    Json(input): Json<SuggestCcRequest>,
) -> Result<Json<SuggestCcResponse>, StatusCode> {
    // Cap input sizes
    if input.body_preview.len() > 50_000 {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    if input.to.is_empty() && input.thread_id.is_none() {
        // No recipients and no thread — can't suggest anything meaningful
        return Ok(Json(SuggestCcResponse {
            suggestions: vec![],
        }));
    }

    let conn = state
        .db
        .get()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Check AI is enabled
    let ai_enabled = conn
        .query_row(
            "SELECT value FROM config WHERE key = 'ai_enabled'",
            [],
            |row| row.get::<_, String>(0),
        )
        .unwrap_or_else(|_| "false".to_string());

    if ai_enabled != "true" || !state.providers.has_providers() {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }

    // Collect all current recipients (to + cc) for exclusion
    let all_recipients: Vec<String> = input
        .to
        .iter()
        .chain(input.cc.iter())
        .map(|e| e.to_lowercase())
        .collect();

    // Query co-occurrences based on To recipients
    let co_occurrences = query_co_occurrences(&conn, &input.to, 10);

    // Get thread CC participants if thread_id is provided
    let thread_cc = if let Some(ref tid) = input.thread_id {
        get_thread_cc_participants(&conn, tid)
            .into_iter()
            .filter(|e| !all_recipients.contains(&e.to_lowercase()))
            .collect::<Vec<_>>()
    } else {
        vec![]
    };

    // If no candidates at all, return empty
    if co_occurrences.is_empty() && thread_cc.is_empty() {
        return Ok(Json(SuggestCcResponse {
            suggestions: vec![],
        }));
    }

    // Build prompt and call AI
    let user_prompt = build_cc_prompt(
        &input.subject,
        &input.body_preview,
        &input.to,
        &input.cc,
        &co_occurrences,
        &thread_cc,
    );

    let ai_response = state
        .providers
        .generate(&user_prompt, Some(CC_SUGGESTION_SYSTEM_PROMPT))
        .await
        .ok_or(StatusCode::BAD_GATEWAY)?;

    let suggestions = parse_suggestions(&ai_response);

    Ok(Json(SuggestCcResponse { suggestions }))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;
    use crate::models::account::{Account, CreateAccount};
    use crate::models::message::InsertMessage;

    fn create_test_account(
        conn: &r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>,
    ) -> Account {
        let input = CreateAccount {
            provider: "gmail".to_string(),
            email: "me@example.com".to_string(),
            display_name: Some("Test User".to_string()),
            imap_host: Some("imap.gmail.com".to_string()),
            imap_port: Some(993),
            smtp_host: Some("smtp.gmail.com".to_string()),
            smtp_port: Some(587),
            username: Some("me@example.com".to_string()),
            password: None,
        };
        Account::create(conn, &input)
    }

    fn make_message(
        account_id: &str,
        thread_id: &str,
        from: &str,
        from_name: Option<&str>,
        msg_id: &str,
        cc: Option<&str>,
    ) -> InsertMessage {
        InsertMessage {
            account_id: account_id.to_string(),
            message_id: Some(format!("<{msg_id}>")),
            thread_id: Some(thread_id.to_string()),
            folder: "INBOX".to_string(),
            from_address: Some(from.to_string()),
            from_name: from_name.map(|n| n.to_string()),
            to_addresses: Some(r#"["me@example.com"]"#.to_string()),
            cc_addresses: cc.map(|c| c.to_string()),
            bcc_addresses: None,
            subject: Some("Test subject".to_string()),
            date: Some(1700000000),
            snippet: Some("test".to_string()),
            body_text: Some("test body".to_string()),
            body_html: None,
            is_read: false,
            is_starred: false,
            is_draft: false,
            labels: None,
            uid: None,
            modseq: None,
            raw_headers: None,
            has_attachments: false,
            attachment_names: None,
            size_bytes: Some(512),
            list_unsubscribe: None,
            list_unsubscribe_post: false,
        }
    }

    // -----------------------------------------------------------------------
    // 1. Co-occurrence query with test data
    // -----------------------------------------------------------------------
    #[test]
    fn test_co_occurrence_basic() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        // Thread 1: alice and bob
        let mut m1 = make_message(&account.id, "t1", "alice@co.com", Some("Alice"), "m1", None);
        m1.uid = Some(1);
        InsertMessage::insert(&conn, &m1);

        let mut m2 = make_message(&account.id, "t1", "bob@co.com", Some("Bob"), "m2", None);
        m2.uid = Some(2);
        InsertMessage::insert(&conn, &m2);

        // Thread 2: alice and bob again
        let mut m3 = make_message(&account.id, "t2", "alice@co.com", Some("Alice"), "m3", None);
        m3.uid = Some(3);
        InsertMessage::insert(&conn, &m3);

        let mut m4 = make_message(&account.id, "t2", "bob@co.com", Some("Bob"), "m4", None);
        m4.uid = Some(4);
        InsertMessage::insert(&conn, &m4);

        let results = query_co_occurrences(&conn, &["alice@co.com".to_string()], 10);
        assert!(!results.is_empty());
        assert_eq!(results[0].email, "bob@co.com");
        assert_eq!(results[0].count, 2);
    }

    // -----------------------------------------------------------------------
    // 2. Co-occurrence excludes current recipients
    // -----------------------------------------------------------------------
    #[test]
    fn test_co_occurrence_excludes_recipients() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        let mut m1 = make_message(&account.id, "t1", "alice@co.com", None, "ex1", None);
        m1.uid = Some(1);
        InsertMessage::insert(&conn, &m1);

        let mut m2 = make_message(&account.id, "t1", "bob@co.com", None, "ex2", None);
        m2.uid = Some(2);
        InsertMessage::insert(&conn, &m2);

        // If both alice and bob are recipients, neither should appear in results
        let results = query_co_occurrences(
            &conn,
            &["alice@co.com".to_string(), "bob@co.com".to_string()],
            10,
        );
        // bob is excluded because he's in the recipients list
        assert!(results.iter().all(|r| r.email != "alice@co.com" && r.email != "bob@co.com"));
    }

    // -----------------------------------------------------------------------
    // 3. Co-occurrence with empty recipients
    // -----------------------------------------------------------------------
    #[test]
    fn test_co_occurrence_empty_recipients() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let _account = create_test_account(&conn);

        let results = query_co_occurrences(&conn, &[], 10);
        assert!(results.is_empty());
    }

    // -----------------------------------------------------------------------
    // 4. Co-occurrence with no matching data
    // -----------------------------------------------------------------------
    #[test]
    fn test_co_occurrence_no_data() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let _account = create_test_account(&conn);

        let results = query_co_occurrences(
            &conn,
            &["nobody@example.com".to_string()],
            10,
        );
        assert!(results.is_empty());
    }

    // -----------------------------------------------------------------------
    // 5. Thread CC participants extraction
    // -----------------------------------------------------------------------
    #[test]
    fn test_thread_cc_participants() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        let mut m1 = make_message(
            &account.id,
            "tcc1",
            "alice@co.com",
            None,
            "cc1",
            Some(r#"["carol@co.com","dave@co.com"]"#),
        );
        m1.uid = Some(1);
        InsertMessage::insert(&conn, &m1);

        let mut m2 = make_message(
            &account.id,
            "tcc1",
            "bob@co.com",
            None,
            "cc2",
            Some(r#"["carol@co.com"]"#),
        );
        m2.uid = Some(2);
        InsertMessage::insert(&conn, &m2);

        let cc = get_thread_cc_participants(&conn, "tcc1");
        assert!(cc.contains(&"carol@co.com".to_string()));
        assert!(cc.contains(&"dave@co.com".to_string()));
        assert_eq!(cc.len(), 2); // carol should be deduplicated
    }

    // -----------------------------------------------------------------------
    // 6. Thread CC participants — no CC in thread
    // -----------------------------------------------------------------------
    #[test]
    fn test_thread_cc_empty() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        let mut m1 = make_message(&account.id, "tcc2", "alice@co.com", None, "cc3", None);
        m1.uid = Some(1);
        InsertMessage::insert(&conn, &m1);

        let cc = get_thread_cc_participants(&conn, "tcc2");
        assert!(cc.is_empty());
    }

    // -----------------------------------------------------------------------
    // 7. AI prompt building
    // -----------------------------------------------------------------------
    #[test]
    fn test_build_cc_prompt() {
        let co_occurrences = vec![
            CoOccurrence {
                email: "bob@co.com".to_string(),
                name: Some("Bob".to_string()),
                count: 5,
            },
            CoOccurrence {
                email: "carol@co.com".to_string(),
                name: None,
                count: 2,
            },
        ];
        let thread_cc = vec!["dave@co.com".to_string()];

        let prompt = build_cc_prompt(
            "Project Review",
            "Please review the attached document",
            &["alice@co.com".to_string()],
            &[],
            &co_occurrences,
            &thread_cc,
        );

        assert!(prompt.contains("Subject: Project Review"));
        assert!(prompt.contains("review the attached document"));
        assert!(prompt.contains("alice@co.com"));
        assert!(prompt.contains("bob@co.com (Bob)"));
        assert!(prompt.contains("5 shared threads"));
        assert!(prompt.contains("carol@co.com"));
        assert!(prompt.contains("dave@co.com"));
        assert!(prompt.contains("Previous CC participants"));
    }

    // -----------------------------------------------------------------------
    // 8. AI prompt building with no co-occurrences
    // -----------------------------------------------------------------------
    #[test]
    fn test_build_cc_prompt_no_candidates() {
        let prompt = build_cc_prompt(
            "Hello",
            "Hi there",
            &["alice@co.com".to_string()],
            &[],
            &[],
            &[],
        );

        assert!(prompt.contains("No frequent co-correspondents found"));
    }

    // -----------------------------------------------------------------------
    // 9. Parse valid AI JSON response
    // -----------------------------------------------------------------------
    #[test]
    fn test_parse_suggestions_valid_json() {
        let response = r#"[
            {"email": "bob@co.com", "name": "Bob", "reason": "Frequent collaborator", "confidence": 0.9, "type": "cc"},
            {"email": "carol@co.com", "name": null, "reason": "Was CC'd before", "confidence": 0.7, "type": "bcc"}
        ]"#;

        let suggestions = parse_suggestions(response);
        assert_eq!(suggestions.len(), 2);
        assert_eq!(suggestions[0].email, "bob@co.com");
        assert_eq!(suggestions[0].confidence, 0.9);
        assert_eq!(suggestions[0].suggestion_type, "cc");
        assert_eq!(suggestions[1].email, "carol@co.com");
        assert_eq!(suggestions[1].suggestion_type, "bcc");
    }

    // -----------------------------------------------------------------------
    // 10. Parse malformed AI JSON response (with markdown wrapper)
    // -----------------------------------------------------------------------
    #[test]
    fn test_parse_suggestions_markdown_wrapped() {
        let response = r#"```json
[{"email": "bob@co.com", "name": "Bob", "reason": "Works closely", "confidence": 0.8, "type": "cc"}]
```"#;

        let suggestions = parse_suggestions(response);
        assert_eq!(suggestions.len(), 1);
        assert_eq!(suggestions[0].email, "bob@co.com");
    }

    // -----------------------------------------------------------------------
    // 11. Parse completely invalid AI response
    // -----------------------------------------------------------------------
    #[test]
    fn test_parse_suggestions_invalid_response() {
        let response = "I'm sorry, I can't help with that.";
        let suggestions = parse_suggestions(response);
        assert!(suggestions.is_empty());
    }

    // -----------------------------------------------------------------------
    // 12. Parse empty array
    // -----------------------------------------------------------------------
    #[test]
    fn test_parse_suggestions_empty_array() {
        let response = "[]";
        let suggestions = parse_suggestions(response);
        assert!(suggestions.is_empty());
    }

    // -----------------------------------------------------------------------
    // 13. Validate suggestions — confidence clamping
    // -----------------------------------------------------------------------
    #[test]
    fn test_validate_suggestions_clamps_confidence() {
        let response = r#"[
            {"email": "a@co.com", "name": null, "reason": "test", "confidence": 1.5, "type": "cc"},
            {"email": "b@co.com", "name": null, "reason": "test", "confidence": -0.3, "type": "cc"}
        ]"#;

        let suggestions = parse_suggestions(response);
        assert_eq!(suggestions.len(), 2);
        assert_eq!(suggestions[0].confidence, 1.0);
        assert_eq!(suggestions[1].confidence, 0.0);
    }

    // -----------------------------------------------------------------------
    // 14. Validate suggestions — invalid type defaults to cc
    // -----------------------------------------------------------------------
    #[test]
    fn test_validate_suggestions_normalizes_type() {
        let response = r#"[
            {"email": "a@co.com", "name": null, "reason": "test", "confidence": 0.5, "type": "invalid"}
        ]"#;

        let suggestions = parse_suggestions(response);
        assert_eq!(suggestions[0].suggestion_type, "cc");
    }

    // -----------------------------------------------------------------------
    // 15. Validate suggestions — truncates to 5
    // -----------------------------------------------------------------------
    #[test]
    fn test_validate_suggestions_max_five() {
        let items: Vec<String> = (0..8)
            .map(|i| {
                format!(
                    r#"{{"email": "u{}@co.com", "name": null, "reason": "test", "confidence": {}, "type": "cc"}}"#,
                    i,
                    0.1 * (8 - i) as f64
                )
            })
            .collect();
        let response = format!("[{}]", items.join(","));

        let suggestions = parse_suggestions(&response);
        assert_eq!(suggestions.len(), 5);
    }

    // -----------------------------------------------------------------------
    // 16. Build prompt truncates long body preview
    // -----------------------------------------------------------------------
    #[test]
    fn test_build_cc_prompt_truncates_body() {
        let long_body = "x".repeat(1000);
        let prompt = build_cc_prompt(
            "Test",
            &long_body,
            &["alice@co.com".to_string()],
            &[],
            &[],
            &[],
        );

        // Body should be truncated to 500 chars + "..."
        assert!(prompt.contains("..."));
        assert!(!prompt.contains(&"x".repeat(600)));
    }

    // -----------------------------------------------------------------------
    // 17. Co-occurrence with multiple threads
    // -----------------------------------------------------------------------
    #[test]
    fn test_co_occurrence_multiple_contacts() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);

        // Thread 1: alice, bob, carol
        for (i, (sender, name)) in [
            ("alice@co.com", "Alice"),
            ("bob@co.com", "Bob"),
            ("carol@co.com", "Carol"),
        ]
        .iter()
        .enumerate()
        {
            let mut m = make_message(
                &account.id,
                "mt1",
                sender,
                Some(name),
                &format!("mt1-{i}"),
                None,
            );
            m.uid = Some(i as i64 + 100);
            InsertMessage::insert(&conn, &m);
        }

        // Thread 2: alice, bob (no carol)
        for (i, (sender, name)) in [("alice@co.com", "Alice"), ("bob@co.com", "Bob")]
            .iter()
            .enumerate()
        {
            let mut m = make_message(
                &account.id,
                "mt2",
                sender,
                Some(name),
                &format!("mt2-{i}"),
                None,
            );
            m.uid = Some(i as i64 + 200);
            InsertMessage::insert(&conn, &m);
        }

        let results = query_co_occurrences(&conn, &["alice@co.com".to_string()], 10);
        // Bob should have count 2 (threads mt1 + mt2), Carol count 1 (mt1 only)
        assert!(results.len() >= 2);
        let bob = results.iter().find(|r| r.email == "bob@co.com").unwrap();
        let carol = results.iter().find(|r| r.email == "carol@co.com").unwrap();
        assert!(bob.count >= carol.count);
    }
}
