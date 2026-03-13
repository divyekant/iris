use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

// ---------------------------------------------------------------------------
// Shared types
// ---------------------------------------------------------------------------

/// Metadata-only version summary (no body) used in list responses.
#[derive(Debug, Serialize)]
pub struct VersionSummary {
    pub id: i64,
    pub draft_id: String,
    pub version_number: i64,
    pub subject: Option<String>,
    pub word_count: i64,
    pub created_at: i64,
}

/// Full version including body fields.
#[derive(Debug, Serialize)]
pub struct VersionDetail {
    pub id: i64,
    pub draft_id: String,
    pub account_id: String,
    pub version_number: i64,
    pub subject: Option<String>,
    pub body: Option<String>,
    pub to_addresses: Option<String>,
    pub cc_addresses: Option<String>,
    pub word_count: i64,
    pub created_at: i64,
}

// ---------------------------------------------------------------------------
// POST /api/drafts/{draft_id}/versions
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct SaveVersionRequest {
    pub account_id: String,
}

pub async fn save_version(
    State(state): State<Arc<AppState>>,
    Path(draft_id): Path<String>,
    Json(req): Json<SaveVersionRequest>,
) -> Result<Json<VersionDetail>, (StatusCode, Json<serde_json::Value>)> {
    let conn = state.db.get().map_err(|_| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "database error"})))
    })?;

    // Verify the draft exists and belongs to the account
    let draft: Option<(Option<String>, Option<String>, Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT subject, body_text, to_addresses, cc_addresses
             FROM messages WHERE id = ?1 AND account_id = ?2 AND is_draft = 1 AND is_deleted = 0",
            rusqlite::params![draft_id, req.account_id],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))
            },
        )
        .ok();

    let (subject, body, to_addresses, cc_addresses) = draft.ok_or_else(|| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "draft not found"})))
    })?;

    let word_count = body
        .as_deref()
        .map(|b| b.split_whitespace().count() as i64)
        .unwrap_or(0);

    // Auto-increment version number
    let next_version: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version_number), 0) + 1 FROM draft_versions WHERE draft_id = ?1",
            rusqlite::params![draft_id],
            |row| row.get(0),
        )
        .unwrap_or(1);

    conn.execute(
        "INSERT INTO draft_versions (draft_id, account_id, version_number, subject, body, to_addresses, cc_addresses, word_count, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, unixepoch())",
        rusqlite::params![
            draft_id,
            req.account_id,
            next_version,
            subject,
            body,
            to_addresses,
            cc_addresses,
            word_count,
        ],
    )
    .map_err(|_| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "failed to save version"})))
    })?;

    let row_id = conn.last_insert_rowid();
    let created_at: i64 = conn
        .query_row(
            "SELECT created_at FROM draft_versions WHERE id = ?1",
            rusqlite::params![row_id],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0)
        });

    Ok(Json(VersionDetail {
        id: row_id,
        draft_id,
        account_id: req.account_id,
        version_number: next_version,
        subject,
        body,
        to_addresses,
        cc_addresses,
        word_count,
        created_at,
    }))
}

// ---------------------------------------------------------------------------
// GET /api/drafts/{draft_id}/versions
// ---------------------------------------------------------------------------

pub async fn list_versions(
    State(state): State<Arc<AppState>>,
    Path(draft_id): Path<String>,
) -> Result<Json<Vec<VersionSummary>>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut stmt = conn
        .prepare(
            "SELECT id, draft_id, version_number, subject, word_count, created_at
             FROM draft_versions WHERE draft_id = ?1
             ORDER BY version_number DESC",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let versions: Vec<VersionSummary> = stmt
        .query_map(rusqlite::params![draft_id], |row| {
            Ok(VersionSummary {
                id: row.get(0)?,
                draft_id: row.get(1)?,
                version_number: row.get(2)?,
                subject: row.get(3)?,
                word_count: row.get(4)?,
                created_at: row.get(5)?,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(versions))
}

// ---------------------------------------------------------------------------
// GET /api/drafts/{draft_id}/versions/{version_number}
// ---------------------------------------------------------------------------

pub async fn get_version(
    State(state): State<Arc<AppState>>,
    Path((draft_id, version_number)): Path<(String, i64)>,
) -> Result<Json<VersionDetail>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let version = conn
        .query_row(
            "SELECT id, draft_id, account_id, version_number, subject, body, to_addresses, cc_addresses, word_count, created_at
             FROM draft_versions WHERE draft_id = ?1 AND version_number = ?2",
            rusqlite::params![draft_id, version_number],
            |row| {
                Ok(VersionDetail {
                    id: row.get(0)?,
                    draft_id: row.get(1)?,
                    account_id: row.get(2)?,
                    version_number: row.get(3)?,
                    subject: row.get(4)?,
                    body: row.get(5)?,
                    to_addresses: row.get(6)?,
                    cc_addresses: row.get(7)?,
                    word_count: row.get(8)?,
                    created_at: row.get(9)?,
                })
            },
        )
        .map_err(|_| StatusCode::NOT_FOUND)?;

    Ok(Json(version))
}

// ---------------------------------------------------------------------------
// POST /api/drafts/{draft_id}/versions/{version_number}/restore
// ---------------------------------------------------------------------------

pub async fn restore_version(
    State(state): State<Arc<AppState>>,
    Path((draft_id, version_number)): Path<(String, i64)>,
) -> Result<Json<VersionDetail>, (StatusCode, Json<serde_json::Value>)> {
    let conn = state.db.get().map_err(|_| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "database error"})))
    })?;

    // Fetch the target version
    let version = conn
        .query_row(
            "SELECT id, account_id, subject, body, to_addresses, cc_addresses, word_count
             FROM draft_versions WHERE draft_id = ?1 AND version_number = ?2",
            rusqlite::params![draft_id, version_number],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, Option<String>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, i64>(6)?,
                ))
            },
        )
        .map_err(|_| {
            (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "version not found"})))
        })?;

    let (_version_id, account_id, subject, body, to_addresses, cc_addresses, _word_count) = version;

    // Update the draft with restored content
    let rows_updated = conn
        .execute(
            "UPDATE messages SET
                subject = ?1,
                body_text = ?2,
                to_addresses = ?3,
                cc_addresses = ?4,
                snippet = ?5,
                updated_at = unixepoch()
             WHERE id = ?6 AND account_id = ?7 AND is_draft = 1 AND is_deleted = 0",
            rusqlite::params![
                subject,
                body,
                to_addresses,
                cc_addresses,
                body.as_deref().map(|b| b.chars().take(200).collect::<String>()),
                draft_id,
                account_id,
            ],
        )
        .map_err(|_| {
            (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "failed to restore draft"})))
        })?;

    if rows_updated == 0 {
        return Err((StatusCode::NOT_FOUND, Json(serde_json::json!({"error": "draft not found or not owned by account"}))));
    }

    // Save the restored state as a new version entry (tagged as a restore)
    let word_count = body
        .as_deref()
        .map(|b| b.split_whitespace().count() as i64)
        .unwrap_or(0);

    let next_version: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version_number), 0) + 1 FROM draft_versions WHERE draft_id = ?1",
            rusqlite::params![draft_id],
            |row| row.get(0),
        )
        .unwrap_or(1);

    conn.execute(
        "INSERT INTO draft_versions (draft_id, account_id, version_number, subject, body, to_addresses, cc_addresses, word_count, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, unixepoch())",
        rusqlite::params![
            draft_id,
            account_id,
            next_version,
            subject,
            body,
            to_addresses,
            cc_addresses,
            word_count,
        ],
    )
    .map_err(|_| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "failed to record restored version"})))
    })?;

    let new_id = conn.last_insert_rowid();
    let created_at: i64 = conn
        .query_row(
            "SELECT created_at FROM draft_versions WHERE id = ?1",
            rusqlite::params![new_id],
            |row| row.get(0),
        )
        .unwrap_or_else(|_| {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0)
        });

    Ok(Json(VersionDetail {
        id: new_id,
        draft_id,
        account_id,
        version_number: next_version,
        subject,
        body,
        to_addresses,
        cc_addresses,
        word_count,
        created_at,
    }))
}

// ---------------------------------------------------------------------------
// GET /api/drafts/{draft_id}/versions/diff?from={n}&to={m}
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct DiffParams {
    pub from: i64,
    pub to: i64,
}

#[derive(Debug, Serialize)]
pub struct DiffLine {
    pub kind: String, // "added", "removed", "unchanged"
    pub text: String,
}

#[derive(Debug, Serialize)]
pub struct DiffResponse {
    pub from_version: i64,
    pub to_version: i64,
    pub from_word_count: i64,
    pub to_word_count: i64,
    pub words_added: i64,
    pub words_removed: i64,
    pub lines: Vec<DiffLine>,
}

pub async fn diff_versions(
    State(state): State<Arc<AppState>>,
    Path(draft_id): Path<String>,
    Query(params): Query<DiffParams>,
) -> Result<Json<DiffResponse>, (StatusCode, Json<serde_json::Value>)> {
    let conn = state.db.get().map_err(|_| {
        (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({"error": "database error"})))
    })?;

    let fetch_version_body = |version_num: i64| -> Result<(Option<String>, i64), StatusCode> {
        conn.query_row(
            "SELECT body, word_count FROM draft_versions WHERE draft_id = ?1 AND version_number = ?2",
            rusqlite::params![draft_id, version_num],
            |row| Ok((row.get::<_, Option<String>>(0)?, row.get::<_, i64>(1)?)),
        )
        .map_err(|_| StatusCode::NOT_FOUND)
    };

    let (from_body, from_wc) = fetch_version_body(params.from).map_err(|_| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": format!("version {} not found", params.from)})))
    })?;

    let (to_body, to_wc) = fetch_version_body(params.to).map_err(|_| {
        (StatusCode::NOT_FOUND, Json(serde_json::json!({"error": format!("version {} not found", params.to)})))
    })?;

    let from_lines: Vec<&str> = from_body.as_deref().unwrap_or("").lines().collect();
    let to_lines: Vec<&str> = to_body.as_deref().unwrap_or("").lines().collect();

    let diff_lines = compute_diff(&from_lines, &to_lines);

    let words_added = diff_lines
        .iter()
        .filter(|l| l.kind == "added")
        .map(|l| l.text.split_whitespace().count() as i64)
        .sum::<i64>();

    let words_removed = diff_lines
        .iter()
        .filter(|l| l.kind == "removed")
        .map(|l| l.text.split_whitespace().count() as i64)
        .sum::<i64>();

    Ok(Json(DiffResponse {
        from_version: params.from,
        to_version: params.to,
        from_word_count: from_wc,
        to_word_count: to_wc,
        words_added,
        words_removed,
        lines: diff_lines,
    }))
}

/// Compute a simple line-level diff using the LCS (longest common subsequence) algorithm.
fn compute_diff(from: &[&str], to: &[&str]) -> Vec<DiffLine> {
    let m = from.len();
    let n = to.len();

    // Build LCS table
    let mut lcs = vec![vec![0usize; n + 1]; m + 1];
    for i in (0..m).rev() {
        for j in (0..n).rev() {
            if from[i] == to[j] {
                lcs[i][j] = 1 + lcs[i + 1][j + 1];
            } else {
                lcs[i][j] = lcs[i + 1][j].max(lcs[i][j + 1]);
            }
        }
    }

    // Trace back through the LCS table to produce diff lines
    let mut result = Vec::new();
    let (mut i, mut j) = (0, 0);

    while i < m || j < n {
        if i < m && j < n && from[i] == to[j] {
            result.push(DiffLine {
                kind: "unchanged".to_string(),
                text: from[i].to_string(),
            });
            i += 1;
            j += 1;
        } else if j < n && (i >= m || lcs[i][j + 1] >= lcs[i + 1][j]) {
            result.push(DiffLine {
                kind: "added".to_string(),
                text: to[j].to_string(),
            });
            j += 1;
        } else {
            result.push(DiffLine {
                kind: "removed".to_string(),
                text: from[i].to_string(),
            });
            i += 1;
        }
    }

    result
}

// ---------------------------------------------------------------------------
// Auto-versioning helper — called by the draft update handler
// ---------------------------------------------------------------------------

/// Saves a new version if `new_body` differs from the current latest version's body.
/// Should be called inside the draft update handler after confirming a body change.
pub fn auto_version_if_changed(
    conn: &r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>,
    draft_id: &str,
    account_id: &str,
    subject: Option<&str>,
    new_body: &str,
    to_addresses: Option<&str>,
    cc_addresses: Option<&str>,
) {
    // Check whether the latest version's body differs from the new body
    let latest_body: Option<String> = conn
        .query_row(
            "SELECT body FROM draft_versions WHERE draft_id = ?1 ORDER BY version_number DESC LIMIT 1",
            rusqlite::params![draft_id],
            |row| row.get(0),
        )
        .ok()
        .flatten();

    let should_save = match latest_body {
        Some(ref lb) => lb.as_str() != new_body,
        None => true, // No versions exist yet — always save the first one
    };

    if !should_save {
        return;
    }

    let word_count = new_body.split_whitespace().count() as i64;

    let next_version: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version_number), 0) + 1 FROM draft_versions WHERE draft_id = ?1",
            rusqlite::params![draft_id],
            |row| row.get(0),
        )
        .unwrap_or(1);

    let _ = conn.execute(
        "INSERT INTO draft_versions (draft_id, account_id, version_number, subject, body, to_addresses, cc_addresses, word_count, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, unixepoch())",
        rusqlite::params![
            draft_id,
            account_id,
            next_version,
            subject,
            new_body,
            to_addresses,
            cc_addresses,
            word_count,
        ],
    );
}

// ---------------------------------------------------------------------------
// Unit tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;
    use crate::models::account::{Account, CreateAccount};
    use crate::models::message;

    type Conn = r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>;

    fn create_test_account(conn: &Conn) -> Account {
        Account::create(
            conn,
            &CreateAccount {
                provider: "gmail".to_string(),
                email: "draft-test@example.com".to_string(),
                display_name: Some("Draft Test".to_string()),
                imap_host: Some("imap.gmail.com".to_string()),
                imap_port: Some(993),
                smtp_host: Some("smtp.gmail.com".to_string()),
                smtp_port: Some(587),
                username: Some("draft-test@example.com".to_string()),
                password: None,
            },
        )
    }

    fn create_test_draft(conn: &Conn, account_id: &str, body: &str) -> String {
        message::save_draft(
            conn,
            None,
            account_id,
            Some(r#"["alice@example.com"]"#),
            None,
            None,
            Some("Test Subject"),
            body,
            None,
        )
    }

    fn insert_version(
        conn: &Conn,
        draft_id: &str,
        account_id: &str,
        version_number: i64,
        body: &str,
    ) -> i64 {
        let word_count = body.split_whitespace().count() as i64;
        conn.execute(
            "INSERT INTO draft_versions (draft_id, account_id, version_number, subject, body, to_addresses, cc_addresses, word_count, created_at)
             VALUES (?1, ?2, ?3, 'Test Subject', ?4, NULL, NULL, ?5, unixepoch())",
            rusqlite::params![draft_id, account_id, version_number, body, word_count],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    #[test]
    fn test_create_version() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);
        let draft_id = create_test_draft(&conn, &account.id, "Initial draft body");

        // Manually insert a version
        let row_id = insert_version(&conn, &draft_id, &account.id, 1, "Initial draft body");
        assert!(row_id > 0);

        // Verify it exists
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM draft_versions WHERE draft_id = ?1",
                rusqlite::params![draft_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_list_versions_ordered_desc() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);
        let draft_id = create_test_draft(&conn, &account.id, "v1");

        insert_version(&conn, &draft_id, &account.id, 1, "Version one body");
        insert_version(&conn, &draft_id, &account.id, 2, "Version two body");
        insert_version(&conn, &draft_id, &account.id, 3, "Version three body");

        let versions: Vec<(i64, i64)> = conn
            .prepare(
                "SELECT version_number, word_count FROM draft_versions WHERE draft_id = ?1 ORDER BY version_number DESC",
            )
            .unwrap()
            .query_map(rusqlite::params![draft_id], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(versions.len(), 3);
        assert_eq!(versions[0].0, 3); // latest first
        assert_eq!(versions[2].0, 1); // oldest last
    }

    #[test]
    fn test_restore_version_updates_draft() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);
        let draft_id = create_test_draft(&conn, &account.id, "Original body");

        insert_version(&conn, &draft_id, &account.id, 1, "Original body");

        // Simulate draft update
        message::save_draft(
            &conn,
            Some(&draft_id),
            &account.id,
            Some(r#"["alice@example.com"]"#),
            None,
            None,
            Some("Updated Subject"),
            "New body after edit",
            None,
        );

        // Restore to version 1
        let restore_body = "Original body";
        let rows = conn
            .execute(
                "UPDATE messages SET body_text = ?1, snippet = ?2, updated_at = unixepoch()
                 WHERE id = ?3 AND is_draft = 1",
                rusqlite::params![
                    restore_body,
                    &restore_body.chars().take(200).collect::<String>(),
                    draft_id,
                ],
            )
            .unwrap();
        assert_eq!(rows, 1);

        // Verify draft has original body
        let body: String = conn
            .query_row(
                "SELECT body_text FROM messages WHERE id = ?1",
                rusqlite::params![draft_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(body, "Original body");
    }

    #[test]
    fn test_diff_two_versions() {
        let from_lines = vec!["Hello world", "This is the old line", "Goodbye"];
        let to_lines = vec!["Hello world", "This is the new line", "Goodbye"];

        let diff = compute_diff(&from_lines, &to_lines);

        let removed: Vec<&str> = diff
            .iter()
            .filter(|l| l.kind == "removed")
            .map(|l| l.text.as_str())
            .collect();
        let added: Vec<&str> = diff
            .iter()
            .filter(|l| l.kind == "added")
            .map(|l| l.text.as_str())
            .collect();
        let unchanged: Vec<&str> = diff
            .iter()
            .filter(|l| l.kind == "unchanged")
            .map(|l| l.text.as_str())
            .collect();

        assert_eq!(removed, vec!["This is the old line"]);
        assert_eq!(added, vec!["This is the new line"]);
        assert_eq!(unchanged, vec!["Hello world", "Goodbye"]);
    }

    #[test]
    fn test_diff_identical_versions() {
        let lines = vec!["Line one", "Line two", "Line three"];
        let diff = compute_diff(&lines, &lines);

        assert!(diff.iter().all(|l| l.kind == "unchanged"));
        assert_eq!(diff.len(), 3);
    }

    #[test]
    fn test_auto_version_if_changed_saves_on_first_call() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);
        let draft_id = create_test_draft(&conn, &account.id, "Body v1");

        auto_version_if_changed(
            &conn,
            &draft_id,
            &account.id,
            Some("Subject"),
            "Body v1",
            None,
            None,
        );

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM draft_versions WHERE draft_id = ?1",
                rusqlite::params![draft_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_auto_version_if_changed_skips_duplicate() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);
        let draft_id = create_test_draft(&conn, &account.id, "Same body");

        // Save once
        auto_version_if_changed(
            &conn,
            &draft_id,
            &account.id,
            Some("Subject"),
            "Same body",
            None,
            None,
        );

        // Save again with same body — should be a no-op
        auto_version_if_changed(
            &conn,
            &draft_id,
            &account.id,
            Some("Subject"),
            "Same body",
            None,
            None,
        );

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM draft_versions WHERE draft_id = ?1",
                rusqlite::params![draft_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1); // Only one version, not two
    }

    #[test]
    fn test_auto_version_if_changed_saves_on_body_change() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);
        let draft_id = create_test_draft(&conn, &account.id, "Body v1");

        auto_version_if_changed(
            &conn,
            &draft_id,
            &account.id,
            Some("Subject"),
            "Body v1",
            None,
            None,
        );

        auto_version_if_changed(
            &conn,
            &draft_id,
            &account.id,
            Some("Subject"),
            "Body v2 — something changed here",
            None,
            None,
        );

        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM draft_versions WHERE draft_id = ?1",
                rusqlite::params![draft_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn test_unique_constraint_on_draft_version() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let account = create_test_account(&conn);
        let draft_id = create_test_draft(&conn, &account.id, "Body");

        insert_version(&conn, &draft_id, &account.id, 1, "Body");

        // Attempting to insert version 1 again should fail due to UNIQUE(draft_id, version_number)
        let result = conn.execute(
            "INSERT INTO draft_versions (draft_id, account_id, version_number, subject, body, word_count, created_at)
             VALUES (?1, ?2, 1, NULL, 'Body', 1, unixepoch())",
            rusqlite::params![draft_id, account.id],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_word_count_accuracy() {
        let body = "Hello world this is a test";
        let wc = body.split_whitespace().count() as i64;
        assert_eq!(wc, 6);

        let empty_body = "";
        let wc_empty = empty_body.split_whitespace().count() as i64;
        assert_eq!(wc_empty, 0);
    }
}
