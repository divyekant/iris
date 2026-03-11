use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

#[derive(Debug, Serialize)]
pub struct ThreadNote {
    pub id: String,
    pub thread_id: String,
    pub content: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Serialize)]
pub struct NotesListResponse {
    pub notes: Vec<ThreadNote>,
}

#[derive(Debug, Deserialize)]
pub struct CreateNoteRequest {
    pub content: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateNoteRequest {
    pub content: String,
}

/// GET /api/threads/{thread_id}/notes
pub async fn list_notes(
    State(state): State<Arc<AppState>>,
    Path(thread_id): Path<String>,
) -> Result<Json<NotesListResponse>, StatusCode> {
    if thread_id.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, thread_id, content, created_at, updated_at
             FROM thread_notes
             WHERE thread_id = ?1
             ORDER BY created_at DESC, rowid DESC",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let notes: Vec<ThreadNote> = stmt
        .query_map(rusqlite::params![thread_id], |row| {
            Ok(ThreadNote {
                id: row.get(0)?,
                thread_id: row.get(1)?,
                content: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(NotesListResponse { notes }))
}

/// POST /api/threads/{thread_id}/notes
pub async fn create_note(
    State(state): State<Arc<AppState>>,
    Path(thread_id): Path<String>,
    Json(req): Json<CreateNoteRequest>,
) -> Result<(StatusCode, Json<ThreadNote>), StatusCode> {
    if thread_id.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let content = req.content.trim().to_string();
    if content.is_empty() || content.len() > 10000 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let note = conn
        .query_row(
            "INSERT INTO thread_notes (thread_id, content)
             VALUES (?1, ?2)
             RETURNING id, thread_id, content, created_at, updated_at",
            rusqlite::params![thread_id, content],
            |row| {
                Ok(ThreadNote {
                    id: row.get(0)?,
                    thread_id: row.get(1)?,
                    content: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            },
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok((StatusCode::CREATED, Json(note)))
}

/// PUT /api/threads/{thread_id}/notes/{id}
pub async fn update_note(
    State(state): State<Arc<AppState>>,
    Path((thread_id, id)): Path<(String, String)>,
    Json(req): Json<UpdateNoteRequest>,
) -> Result<Json<ThreadNote>, StatusCode> {
    if thread_id.is_empty() || id.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let content = req.content.trim().to_string();
    if content.is_empty() || content.len() > 10000 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let note = conn
        .query_row(
            "UPDATE thread_notes SET content = ?1, updated_at = unixepoch()
             WHERE id = ?2 AND thread_id = ?3
             RETURNING id, thread_id, content, created_at, updated_at",
            rusqlite::params![content, id, thread_id],
            |row| {
                Ok(ThreadNote {
                    id: row.get(0)?,
                    thread_id: row.get(1)?,
                    content: row.get(2)?,
                    created_at: row.get(3)?,
                    updated_at: row.get(4)?,
                })
            },
        )
        .map_err(|e| {
            if matches!(e, rusqlite::Error::QueryReturnedNoRows) {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        })?;

    Ok(Json(note))
}

/// DELETE /api/threads/{thread_id}/notes/{id}
pub async fn delete_note(
    State(state): State<Arc<AppState>>,
    Path((thread_id, id)): Path<(String, String)>,
) -> Result<StatusCode, StatusCode> {
    if thread_id.is_empty() || id.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let rows = conn
        .execute(
            "DELETE FROM thread_notes WHERE id = ?1 AND thread_id = ?2",
            rusqlite::params![id, thread_id],
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if rows == 0 {
        return Err(StatusCode::NOT_FOUND);
    }

    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use crate::db::create_test_pool;

    #[test]
    fn test_create_and_list_notes() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        // Create a note
        let note_id: String = conn
            .query_row(
                "INSERT INTO thread_notes (thread_id, content)
                 VALUES ('thread-1', 'Test note content')
                 RETURNING id",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!note_id.is_empty());

        // Create a second note
        conn.execute(
            "INSERT INTO thread_notes (thread_id, content) VALUES ('thread-1', 'Second note')",
            [],
        )
        .unwrap();

        // Create a note for a different thread
        conn.execute(
            "INSERT INTO thread_notes (thread_id, content) VALUES ('thread-2', 'Other thread note')",
            [],
        )
        .unwrap();

        // List notes for thread-1 (should return 2, ordered by created_at DESC)
        let mut stmt = conn
            .prepare(
                "SELECT id, thread_id, content FROM thread_notes
                 WHERE thread_id = ?1 ORDER BY created_at DESC, rowid DESC",
            )
            .unwrap();
        let notes: Vec<(String, String, String)> = stmt
            .query_map(rusqlite::params!["thread-1"], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].2, "Second note");
        assert_eq!(notes[1].2, "Test note content");
    }

    #[test]
    fn test_update_note() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let note_id: String = conn
            .query_row(
                "INSERT INTO thread_notes (thread_id, content)
                 VALUES ('thread-1', 'Original content')
                 RETURNING id",
                [],
                |row| row.get(0),
            )
            .unwrap();

        let updated_content: String = conn
            .query_row(
                "UPDATE thread_notes SET content = 'Updated content', updated_at = unixepoch()
                 WHERE id = ?1 AND thread_id = 'thread-1'
                 RETURNING content",
                rusqlite::params![note_id],
                |row| row.get(0),
            )
            .unwrap();

        assert_eq!(updated_content, "Updated content");
    }

    #[test]
    fn test_update_note_wrong_thread() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let note_id: String = conn
            .query_row(
                "INSERT INTO thread_notes (thread_id, content)
                 VALUES ('thread-1', 'Content')
                 RETURNING id",
                [],
                |row| row.get(0),
            )
            .unwrap();

        // Try updating with wrong thread_id — should fail (no rows returned)
        let result = conn.query_row(
            "UPDATE thread_notes SET content = 'Hacked'
             WHERE id = ?1 AND thread_id = 'wrong-thread'
             RETURNING id",
            rusqlite::params![note_id],
            |row| row.get::<_, String>(0),
        );

        assert!(result.is_err());
    }

    #[test]
    fn test_delete_note() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let note_id: String = conn
            .query_row(
                "INSERT INTO thread_notes (thread_id, content)
                 VALUES ('thread-1', 'To delete')
                 RETURNING id",
                [],
                |row| row.get(0),
            )
            .unwrap();

        let rows = conn
            .execute(
                "DELETE FROM thread_notes WHERE id = ?1 AND thread_id = 'thread-1'",
                rusqlite::params![note_id],
            )
            .unwrap();
        assert_eq!(rows, 1);

        // Verify gone
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM thread_notes WHERE id = ?1",
                rusqlite::params![note_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_delete_note_not_found() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let rows = conn
            .execute(
                "DELETE FROM thread_notes WHERE id = 'nonexistent' AND thread_id = 'thread-1'",
                [],
            )
            .unwrap();
        assert_eq!(rows, 0);
    }

    #[test]
    fn test_content_max_length() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        // SQLite doesn't enforce length, but the API will — verify the DB accepts long content
        let long_content = "x".repeat(10000);
        let result = conn.execute(
            "INSERT INTO thread_notes (thread_id, content) VALUES ('thread-1', ?1)",
            rusqlite::params![long_content],
        );
        assert!(result.is_ok());
    }
}
