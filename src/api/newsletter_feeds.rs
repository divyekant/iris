use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::AppState;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct NewsletterFeed {
    pub id: i64,
    pub account_id: String,
    pub sender_address: String,
    pub sender_name: Option<String>,
    pub display_name: Option<String>,
    pub is_muted: bool,
    pub is_favorite: bool,
    pub last_received_at: Option<String>,
    pub article_count: i64,
    pub unread_count: i64,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct FeedArticle {
    pub id: String,
    pub message_id: Option<String>,
    pub subject: Option<String>,
    pub snippet: Option<String>,
    pub date: Option<i64>,
    pub is_read: bool,
    pub has_attachments: bool,
}

#[derive(Debug, Serialize)]
pub struct ListFeedsResponse {
    pub feeds: Vec<NewsletterFeed>,
}

#[derive(Debug, Serialize)]
pub struct ListArticlesResponse {
    pub articles: Vec<FeedArticle>,
    pub total: i64,
}

#[derive(Debug, Serialize)]
pub struct DiscoverResponse {
    pub discovered: usize,
    pub total_feeds: usize,
}

#[derive(Debug, Deserialize)]
pub struct UpdateFeedRequest {
    pub display_name: Option<String>,
    pub is_muted: Option<bool>,
    pub is_favorite: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub struct ArticlesQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct ListFeedsQuery {
    pub account_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Row mapper
// ---------------------------------------------------------------------------

fn feed_from_row(row: &rusqlite::Row) -> rusqlite::Result<NewsletterFeed> {
    Ok(NewsletterFeed {
        id: row.get(0)?,
        account_id: row.get(1)?,
        sender_address: row.get(2)?,
        sender_name: row.get(3)?,
        display_name: row.get(4)?,
        is_muted: row.get::<_, i64>(5)? != 0,
        is_favorite: row.get::<_, i64>(6)? != 0,
        last_received_at: row.get(7)?,
        article_count: row.get(8)?,
        created_at: row.get(9)?,
        unread_count: row.get(10)?,
    })
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// GET /api/newsletter-feeds — list all newsletter feeds with unread counts
pub async fn list_feeds(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListFeedsQuery>,
) -> Result<Json<ListFeedsResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let (query, bind_account) = if let Some(ref acct) = params.account_id {
        (
            "SELECT nf.id, nf.account_id, nf.sender_address, nf.sender_name,
                    nf.display_name, nf.is_muted, nf.is_favorite, nf.last_received_at,
                    nf.article_count, nf.created_at,
                    COALESCE((
                        SELECT COUNT(*) FROM messages m
                        WHERE m.from_address = nf.sender_address
                          AND m.account_id = nf.account_id
                          AND m.is_read = 0
                          AND m.is_deleted = 0
                    ), 0) as unread_count
             FROM newsletter_feeds nf
             WHERE nf.account_id = ?1
             ORDER BY nf.is_favorite DESC, nf.last_received_at DESC",
            Some(acct.clone()),
        )
    } else {
        (
            "SELECT nf.id, nf.account_id, nf.sender_address, nf.sender_name,
                    nf.display_name, nf.is_muted, nf.is_favorite, nf.last_received_at,
                    nf.article_count, nf.created_at,
                    COALESCE((
                        SELECT COUNT(*) FROM messages m
                        WHERE m.from_address = nf.sender_address
                          AND m.account_id = nf.account_id
                          AND m.is_read = 0
                          AND m.is_deleted = 0
                    ), 0) as unread_count
             FROM newsletter_feeds nf
             ORDER BY nf.is_favorite DESC, nf.last_received_at DESC",
            None,
        )
    };

    let mut stmt = conn.prepare(query).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let feeds: Vec<NewsletterFeed> = if let Some(ref acct) = bind_account {
        let rows = stmt
            .query_map(rusqlite::params![acct], feed_from_row)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        rows.filter_map(|r| r.ok()).collect()
    } else {
        let rows = stmt
            .query_map([], feed_from_row)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        rows.filter_map(|r| r.ok()).collect()
    };

    Ok(Json(ListFeedsResponse { feeds }))
}

/// GET /api/newsletter-feeds/:id/articles — paginated articles for a feed
pub async fn list_articles(
    State(state): State<Arc<AppState>>,
    Path(feed_id): Path<i64>,
    Query(params): Query<ArticlesQuery>,
) -> Result<Json<ListArticlesResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let limit = params.limit.unwrap_or(20).min(100);
    let offset = params.offset.unwrap_or(0).max(0);

    // Look up the feed to get sender_address and account_id
    let feed: (String, String) = conn
        .query_row(
            "SELECT sender_address, account_id FROM newsletter_feeds WHERE id = ?1",
            rusqlite::params![feed_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let (sender_address, account_id) = feed;

    let mut stmt = conn
        .prepare(
            "SELECT m.id, m.message_id, m.subject, m.snippet, m.date, m.is_read, m.has_attachments
             FROM messages m
             WHERE m.from_address = ?1
               AND m.account_id = ?2
               AND m.is_deleted = 0
             ORDER BY m.date DESC
             LIMIT ?3 OFFSET ?4",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let articles: Vec<FeedArticle> = stmt
        .query_map(
            rusqlite::params![sender_address, account_id, limit, offset],
            |row| {
                Ok(FeedArticle {
                    id: row.get(0)?,
                    message_id: row.get(1)?,
                    subject: row.get(2)?,
                    snippet: row.get(3)?,
                    date: row.get(4)?,
                    is_read: row.get::<_, i64>(5)? != 0,
                    has_attachments: row.get::<_, i64>(6)? != 0,
                })
            },
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    let total: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM messages
             WHERE from_address = ?1 AND account_id = ?2 AND is_deleted = 0",
            rusqlite::params![sender_address, account_id],
            |row| row.get(0),
        )
        .unwrap_or(0);

    Ok(Json(ListArticlesResponse { articles, total }))
}

/// POST /api/newsletter-feeds/discover — auto-discover newsletter senders
pub async fn discover_feeds(
    State(state): State<Arc<AppState>>,
) -> Result<Json<DiscoverResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Find newsletter senders: List-Unsubscribe present, or ai_category in (Newsletters, Promotions)
    let mut stmt = conn
        .prepare(
            "SELECT m.account_id, m.from_address, m.from_name,
                    MAX(datetime(m.date, 'unixepoch')) as last_date,
                    COUNT(*) as cnt
             FROM messages m
             WHERE m.is_deleted = 0
               AND (
                   m.list_unsubscribe IS NOT NULL
                   OR m.list_unsubscribe_post = 1
                   OR m.ai_category IN ('Newsletters', 'Promotions')
               )
               AND m.from_address IS NOT NULL
             GROUP BY m.account_id, m.from_address
             ORDER BY cnt DESC",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    struct DiscoveredFeed {
        account_id: String,
        sender_address: String,
        sender_name: Option<String>,
        last_received_at: Option<String>,
        article_count: i64,
    }

    let discovered: Vec<DiscoveredFeed> = stmt
        .query_map([], |row| {
            Ok(DiscoveredFeed {
                account_id: row.get(0)?,
                sender_address: row.get(1)?,
                sender_name: row.get(2)?,
                last_received_at: row.get(3)?,
                article_count: row.get(4)?,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    let mut created = 0usize;
    for feed in &discovered {
        let rows = conn
            .execute(
                "INSERT OR IGNORE INTO newsletter_feeds
                    (account_id, sender_address, sender_name, last_received_at, article_count)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![
                    feed.account_id,
                    feed.sender_address,
                    feed.sender_name,
                    feed.last_received_at,
                    feed.article_count,
                ],
            )
            .unwrap_or(0);
        if rows > 0 {
            created += 1;
        }
    }

    // Also update existing feeds' article counts and last_received_at
    let _ = conn.execute_batch(
        "UPDATE newsletter_feeds SET
            article_count = COALESCE((
                SELECT COUNT(*) FROM messages m
                WHERE m.from_address = newsletter_feeds.sender_address
                  AND m.account_id = newsletter_feeds.account_id
                  AND m.is_deleted = 0
            ), 0),
            last_received_at = COALESCE((
                SELECT MAX(datetime(m.date, 'unixepoch')) FROM messages m
                WHERE m.from_address = newsletter_feeds.sender_address
                  AND m.account_id = newsletter_feeds.account_id
                  AND m.is_deleted = 0
            ), newsletter_feeds.last_received_at)",
    );

    let total_feeds: usize = conn
        .query_row("SELECT COUNT(*) FROM newsletter_feeds", [], |row| {
            row.get::<_, i64>(0)
        })
        .unwrap_or(0) as usize;

    Ok(Json(DiscoverResponse {
        discovered: created,
        total_feeds,
    }))
}

/// PUT /api/newsletter-feeds/:id — update feed settings
pub async fn update_feed(
    State(state): State<Arc<AppState>>,
    Path(feed_id): Path<i64>,
    Json(req): Json<UpdateFeedRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Verify feed exists
    let exists: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM newsletter_feeds WHERE id = ?1",
            rusqlite::params![feed_id],
            |row| row.get::<_, i64>(0),
        )
        .map(|c| c > 0)
        .unwrap_or(false);

    if !exists {
        return Err(StatusCode::NOT_FOUND);
    }

    // Build dynamic UPDATE
    let mut updates = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut idx = 1;

    if let Some(ref name) = req.display_name {
        updates.push(format!("display_name = ?{idx}"));
        params.push(Box::new(name.clone()));
        idx += 1;
    }
    if let Some(muted) = req.is_muted {
        updates.push(format!("is_muted = ?{idx}"));
        params.push(Box::new(muted as i64));
        idx += 1;
    }
    if let Some(fav) = req.is_favorite {
        updates.push(format!("is_favorite = ?{idx}"));
        params.push(Box::new(fav as i64));
        idx += 1;
    }

    if updates.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    params.push(Box::new(feed_id));
    let sql = format!(
        "UPDATE newsletter_feeds SET {} WHERE id = ?{idx}",
        updates.join(", ")
    );

    conn.execute(&sql, rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Return updated feed
    let feed = conn
        .query_row(
            "SELECT id, display_name, is_muted, is_favorite FROM newsletter_feeds WHERE id = ?1",
            rusqlite::params![feed_id],
            |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, i64>(0)?,
                    "display_name": row.get::<_, Option<String>>(1)?,
                    "is_muted": row.get::<_, i64>(2)? != 0,
                    "is_favorite": row.get::<_, i64>(3)? != 0,
                }))
            },
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(feed))
}

/// POST /api/newsletter-feeds/:id/mark-read — mark all articles in feed as read
pub async fn mark_feed_read(
    State(state): State<Arc<AppState>>,
    Path(feed_id): Path<i64>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Look up feed
    let feed: (String, String) = conn
        .query_row(
            "SELECT sender_address, account_id FROM newsletter_feeds WHERE id = ?1",
            rusqlite::params![feed_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let (sender_address, account_id) = feed;

    let updated = conn
        .execute(
            "UPDATE messages SET is_read = 1, updated_at = unixepoch()
             WHERE from_address = ?1 AND account_id = ?2 AND is_read = 0 AND is_deleted = 0",
            rusqlite::params![sender_address, account_id],
        )
        .unwrap_or(0);

    Ok(Json(serde_json::json!({ "marked_read": updated })))
}

/// DELETE /api/newsletter-feeds/:id — remove a feed (keeps emails)
pub async fn delete_feed(
    State(state): State<Arc<AppState>>,
    Path(feed_id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let deleted = conn
        .execute(
            "DELETE FROM newsletter_feeds WHERE id = ?1",
            rusqlite::params![feed_id],
        )
        .unwrap_or(0);

    if deleted > 0 {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;

    /// Insert a test account and return its id
    fn setup_account(conn: &rusqlite::Connection) -> String {
        conn.execute(
            "INSERT INTO accounts (id, provider, email) VALUES ('acc1', 'imap', 'user@example.com')",
            [],
        )
        .unwrap();
        "acc1".to_string()
    }

    /// Insert a newsletter-style message with list_unsubscribe set
    fn insert_newsletter_message(
        conn: &rusqlite::Connection,
        account_id: &str,
        msg_id: &str,
        sender: &str,
        sender_name: &str,
        subject: &str,
        date: i64,
        is_read: bool,
        list_unsubscribe: Option<&str>,
        ai_category: Option<&str>,
    ) {
        conn.execute(
            "INSERT INTO messages (
                id, account_id, message_id, folder, from_address, from_name,
                subject, snippet, body_text, date, is_read, list_unsubscribe, ai_category
            ) VALUES (?1, ?2, ?3, 'INBOX', ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            rusqlite::params![
                msg_id,
                account_id,
                format!("<{msg_id}@example.com>"),
                sender,
                sender_name,
                subject,
                &subject[..subject.len().min(50)],
                format!("Body of {subject}"),
                date,
                is_read as i64,
                list_unsubscribe,
                ai_category,
            ],
        )
        .unwrap();
    }

    // -- 1. Discovery: finds newsletters by list_unsubscribe -------------------

    #[test]
    fn test_discover_by_list_unsubscribe() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let acct = setup_account(&conn);

        insert_newsletter_message(
            &conn, &acct, "nl1", "news@pub.com", "The Publisher",
            "Issue #1", 1700000000, false,
            Some("<mailto:unsub@pub.com>"), None,
        );
        insert_newsletter_message(
            &conn, &acct, "nl2", "news@pub.com", "The Publisher",
            "Issue #2", 1700100000, false,
            Some("<mailto:unsub@pub.com>"), None,
        );

        // Run discover
        let discovered = discover_newsletter_feeds(&conn);
        assert_eq!(discovered.len(), 1);
        assert_eq!(discovered[0].0, "news@pub.com");
        assert_eq!(discovered[0].1, 2); // 2 articles
    }

    // -- 2. Discovery: finds newsletters by ai_category -----------------------

    #[test]
    fn test_discover_by_ai_category() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let acct = setup_account(&conn);

        insert_newsletter_message(
            &conn, &acct, "promo1", "deals@shop.com", "Shop Deals",
            "50% off!", 1700000000, false,
            None, Some("Promotions"),
        );

        let discovered = discover_newsletter_feeds(&conn);
        assert_eq!(discovered.len(), 1);
        assert_eq!(discovered[0].0, "deals@shop.com");
    }

    // -- 3. Discovery: ignores non-newsletter messages -------------------------

    #[test]
    fn test_discover_ignores_regular_messages() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let acct = setup_account(&conn);

        // Regular message: no list_unsubscribe, ai_category = Primary
        insert_newsletter_message(
            &conn, &acct, "regular1", "bob@work.com", "Bob",
            "Meeting notes", 1700000000, false,
            None, Some("Primary"),
        );

        let discovered = discover_newsletter_feeds(&conn);
        assert_eq!(discovered.len(), 0);
    }

    // -- 4. Discovery: creates feeds via INSERT OR IGNORE ----------------------

    #[test]
    fn test_discover_creates_feed_rows() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let acct = setup_account(&conn);

        insert_newsletter_message(
            &conn, &acct, "nl1", "news@pub.com", "The Publisher",
            "Issue #1", 1700000000, false,
            Some("<mailto:unsub@pub.com>"), None,
        );

        // First discover
        let created = run_discover(&conn);
        assert!(created > 0);

        // Second discover should not duplicate
        let created2 = run_discover(&conn);
        assert_eq!(created2, 0);

        let total: i64 = conn
            .query_row("SELECT COUNT(*) FROM newsletter_feeds", [], |row| row.get(0))
            .unwrap();
        assert_eq!(total, 1);
    }

    // -- 5. List feeds: returns empty when none exist --------------------------

    #[test]
    fn test_list_feeds_empty() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let feeds = query_feeds(&conn, None);
        assert_eq!(feeds.len(), 0);
    }

    // -- 6. List feeds: returns feeds with unread counts -----------------------

    #[test]
    fn test_list_feeds_with_unread_count() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let acct = setup_account(&conn);

        insert_newsletter_message(
            &conn, &acct, "nl1", "news@pub.com", "Pub",
            "Issue #1", 1700000000, false,
            Some("<unsub>"), None,
        );
        insert_newsletter_message(
            &conn, &acct, "nl2", "news@pub.com", "Pub",
            "Issue #2", 1700100000, true,
            Some("<unsub>"), None,
        );

        run_discover(&conn);
        let feeds = query_feeds(&conn, Some(&acct));
        assert_eq!(feeds.len(), 1);
        assert_eq!(feeds[0].sender_address, "news@pub.com");
        assert_eq!(feeds[0].unread_count, 1); // 1 unread out of 2
        assert_eq!(feeds[0].article_count, 2);
    }

    // -- 7. List feeds: filters by account_id ----------------------------------

    #[test]
    fn test_list_feeds_filter_by_account() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        setup_account(&conn);
        conn.execute(
            "INSERT INTO accounts (id, provider, email) VALUES ('acc2', 'imap', 'other@example.com')",
            [],
        ).unwrap();

        insert_newsletter_message(
            &conn, "acc1", "nl1", "news@pub.com", "Pub",
            "Issue #1", 1700000000, false,
            Some("<unsub>"), None,
        );
        insert_newsletter_message(
            &conn, "acc2", "nl2", "other@news.com", "Other",
            "Other Issue", 1700000000, false,
            Some("<unsub>"), None,
        );

        run_discover(&conn);

        let feeds_acc1 = query_feeds(&conn, Some("acc1"));
        assert_eq!(feeds_acc1.len(), 1);
        assert_eq!(feeds_acc1[0].sender_address, "news@pub.com");

        let feeds_acc2 = query_feeds(&conn, Some("acc2"));
        assert_eq!(feeds_acc2.len(), 1);
        assert_eq!(feeds_acc2[0].sender_address, "other@news.com");

        let all_feeds = query_feeds(&conn, None);
        assert_eq!(all_feeds.len(), 2);
    }

    // -- 8. Articles: returns paginated articles for a feed --------------------

    #[test]
    fn test_list_articles() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let acct = setup_account(&conn);

        for i in 0..5 {
            insert_newsletter_message(
                &conn, &acct, &format!("nl{i}"), "news@pub.com", "Pub",
                &format!("Issue #{i}"), 1700000000 + i * 100000, false,
                Some("<unsub>"), None,
            );
        }

        run_discover(&conn);
        let feed_id = get_feed_id(&conn, "news@pub.com");

        let (articles, total) = query_articles(&conn, feed_id, 3, 0);
        assert_eq!(total, 5);
        assert_eq!(articles.len(), 3);
        // Should be ordered by date DESC
        assert!(articles[0].date.unwrap() > articles[1].date.unwrap());
    }

    // -- 9. Articles: pagination offset works ----------------------------------

    #[test]
    fn test_articles_pagination() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let acct = setup_account(&conn);

        for i in 0..5 {
            insert_newsletter_message(
                &conn, &acct, &format!("nl{i}"), "news@pub.com", "Pub",
                &format!("Issue #{i}"), 1700000000 + i * 100000, false,
                Some("<unsub>"), None,
            );
        }

        run_discover(&conn);
        let feed_id = get_feed_id(&conn, "news@pub.com");

        let (page1, _) = query_articles(&conn, feed_id, 2, 0);
        let (page2, _) = query_articles(&conn, feed_id, 2, 2);
        assert_eq!(page1.len(), 2);
        assert_eq!(page2.len(), 2);
        // Pages should not overlap
        assert_ne!(page1[0].id, page2[0].id);
    }

    // -- 10. Update feed: change display_name ----------------------------------

    #[test]
    fn test_update_feed_display_name() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let acct = setup_account(&conn);

        insert_newsletter_message(
            &conn, &acct, "nl1", "news@pub.com", "Pub",
            "Issue #1", 1700000000, false,
            Some("<unsub>"), None,
        );
        run_discover(&conn);
        let feed_id = get_feed_id(&conn, "news@pub.com");

        // Update display_name
        conn.execute(
            "UPDATE newsletter_feeds SET display_name = ?1 WHERE id = ?2",
            rusqlite::params!["My Favorite Newsletter", feed_id],
        ).unwrap();

        let name: Option<String> = conn
            .query_row(
                "SELECT display_name FROM newsletter_feeds WHERE id = ?1",
                rusqlite::params![feed_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(name, Some("My Favorite Newsletter".to_string()));
    }

    // -- 11. Update feed: toggle is_muted and is_favorite ----------------------

    #[test]
    fn test_update_feed_muted_favorite() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let acct = setup_account(&conn);

        insert_newsletter_message(
            &conn, &acct, "nl1", "news@pub.com", "Pub",
            "Issue #1", 1700000000, false,
            Some("<unsub>"), None,
        );
        run_discover(&conn);
        let feed_id = get_feed_id(&conn, "news@pub.com");

        // Mute
        conn.execute(
            "UPDATE newsletter_feeds SET is_muted = 1 WHERE id = ?1",
            rusqlite::params![feed_id],
        ).unwrap();

        let muted: i64 = conn
            .query_row(
                "SELECT is_muted FROM newsletter_feeds WHERE id = ?1",
                rusqlite::params![feed_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(muted, 1);

        // Favorite
        conn.execute(
            "UPDATE newsletter_feeds SET is_favorite = 1 WHERE id = ?1",
            rusqlite::params![feed_id],
        ).unwrap();

        let fav: i64 = conn
            .query_row(
                "SELECT is_favorite FROM newsletter_feeds WHERE id = ?1",
                rusqlite::params![feed_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(fav, 1);
    }

    // -- 12. Mark read: marks all unread articles as read ----------------------

    #[test]
    fn test_mark_feed_read() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let acct = setup_account(&conn);

        insert_newsletter_message(
            &conn, &acct, "nl1", "news@pub.com", "Pub",
            "Issue #1", 1700000000, false,
            Some("<unsub>"), None,
        );
        insert_newsletter_message(
            &conn, &acct, "nl2", "news@pub.com", "Pub",
            "Issue #2", 1700100000, false,
            Some("<unsub>"), None,
        );
        insert_newsletter_message(
            &conn, &acct, "nl3", "news@pub.com", "Pub",
            "Issue #3", 1700200000, true, // already read
            Some("<unsub>"), None,
        );

        run_discover(&conn);
        let feed_id = get_feed_id(&conn, "news@pub.com");

        // Mark all as read
        let (sender, account) = get_feed_sender(&conn, feed_id);
        let updated = conn
            .execute(
                "UPDATE messages SET is_read = 1, updated_at = unixepoch()
                 WHERE from_address = ?1 AND account_id = ?2 AND is_read = 0 AND is_deleted = 0",
                rusqlite::params![sender, account],
            )
            .unwrap();
        assert_eq!(updated, 2); // 2 were unread

        // Verify all are now read
        let unread: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE from_address = ?1 AND account_id = ?2 AND is_read = 0",
                rusqlite::params![sender, account],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(unread, 0);
    }

    // -- 13. Delete feed: removes feed but keeps messages ----------------------

    #[test]
    fn test_delete_feed_keeps_messages() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let acct = setup_account(&conn);

        insert_newsletter_message(
            &conn, &acct, "nl1", "news@pub.com", "Pub",
            "Issue #1", 1700000000, false,
            Some("<unsub>"), None,
        );
        run_discover(&conn);
        let feed_id = get_feed_id(&conn, "news@pub.com");

        // Delete feed
        let deleted = conn
            .execute("DELETE FROM newsletter_feeds WHERE id = ?1", rusqlite::params![feed_id])
            .unwrap();
        assert_eq!(deleted, 1);

        // Feed is gone
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM newsletter_feeds", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);

        // Message still exists
        let msg_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM messages WHERE id = 'nl1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(msg_count, 1);
    }

    // -- 14. Delete non-existent feed returns 0 rows ---------------------------

    #[test]
    fn test_delete_nonexistent_feed() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let deleted = conn
            .execute("DELETE FROM newsletter_feeds WHERE id = ?1", rusqlite::params![9999])
            .unwrap();
        assert_eq!(deleted, 0);
    }

    // -- 15. Discovery deduplication across runs --------------------------------

    #[test]
    fn test_discover_idempotent() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let acct = setup_account(&conn);

        insert_newsletter_message(
            &conn, &acct, "nl1", "news@pub.com", "Pub",
            "Issue #1", 1700000000, false,
            Some("<unsub>"), None,
        );

        let first = run_discover(&conn);
        assert_eq!(first, 1);

        // Add another message from same sender
        insert_newsletter_message(
            &conn, &acct, "nl2", "news@pub.com", "Pub",
            "Issue #2", 1700100000, false,
            Some("<unsub>"), None,
        );

        let second = run_discover(&conn);
        assert_eq!(second, 0); // no new feeds created (same sender)

        // But article_count should be updated
        let feed_id = get_feed_id(&conn, "news@pub.com");
        let count: i64 = conn
            .query_row(
                "SELECT article_count FROM newsletter_feeds WHERE id = ?1",
                rusqlite::params![feed_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);
    }

    // -- 16. Articles for non-existent feed returns error ----------------------

    #[test]
    fn test_articles_nonexistent_feed() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let result = conn.query_row(
            "SELECT sender_address, account_id FROM newsletter_feeds WHERE id = ?1",
            rusqlite::params![9999_i64],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        );
        assert!(result.is_err());
    }

    // -- 17. Favorites sort before non-favorites --------------------------------

    #[test]
    fn test_favorites_sort_first() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let acct = setup_account(&conn);

        insert_newsletter_message(
            &conn, &acct, "nl1", "alpha@pub.com", "Alpha",
            "Alpha Issue", 1700200000, false,
            Some("<unsub>"), None,
        );
        insert_newsletter_message(
            &conn, &acct, "nl2", "beta@pub.com", "Beta",
            "Beta Issue", 1700100000, false,
            Some("<unsub>"), None,
        );

        run_discover(&conn);

        // Make beta a favorite
        let beta_id = get_feed_id(&conn, "beta@pub.com");
        conn.execute(
            "UPDATE newsletter_feeds SET is_favorite = 1 WHERE id = ?1",
            rusqlite::params![beta_id],
        ).unwrap();

        let feeds = query_feeds(&conn, Some(&acct));
        assert_eq!(feeds.len(), 2);
        // Beta (favorite) should be first despite older last_received_at
        assert_eq!(feeds[0].sender_address, "beta@pub.com");
        assert!(feeds[0].is_favorite);
    }

    // -------------------------------------------------------------------------
    // Test helpers: run DB-level operations matching the handler logic
    // -------------------------------------------------------------------------

    fn discover_newsletter_feeds(conn: &rusqlite::Connection) -> Vec<(String, i64)> {
        let mut stmt = conn
            .prepare(
                "SELECT m.from_address, COUNT(*) as cnt
                 FROM messages m
                 WHERE m.is_deleted = 0
                   AND (
                       m.list_unsubscribe IS NOT NULL
                       OR m.list_unsubscribe_post = 1
                       OR m.ai_category IN ('Newsletters', 'Promotions')
                   )
                   AND m.from_address IS NOT NULL
                 GROUP BY m.account_id, m.from_address",
            )
            .unwrap();

        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect()
    }

    /// Run discover logic and return count of newly created feeds
    fn run_discover(conn: &rusqlite::Connection) -> usize {
        let mut stmt = conn
            .prepare(
                "SELECT m.account_id, m.from_address, m.from_name,
                        MAX(datetime(m.date, 'unixepoch')) as last_date,
                        COUNT(*) as cnt
                 FROM messages m
                 WHERE m.is_deleted = 0
                   AND (
                       m.list_unsubscribe IS NOT NULL
                       OR m.list_unsubscribe_post = 1
                       OR m.ai_category IN ('Newsletters', 'Promotions')
                   )
                   AND m.from_address IS NOT NULL
                 GROUP BY m.account_id, m.from_address
                 ORDER BY cnt DESC",
            )
            .unwrap();

        let discovered: Vec<(String, String, Option<String>, Option<String>, i64)> = stmt
            .query_map([], |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                ))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        let mut created = 0usize;
        for (account_id, sender, name, last_date, count) in &discovered {
            let rows = conn
                .execute(
                    "INSERT OR IGNORE INTO newsletter_feeds
                        (account_id, sender_address, sender_name, last_received_at, article_count)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![account_id, sender, name, last_date, count],
                )
                .unwrap_or(0);
            if rows > 0 {
                created += 1;
            }
        }

        // Update existing feeds
        let _ = conn.execute_batch(
            "UPDATE newsletter_feeds SET
                article_count = COALESCE((
                    SELECT COUNT(*) FROM messages m
                    WHERE m.from_address = newsletter_feeds.sender_address
                      AND m.account_id = newsletter_feeds.account_id
                      AND m.is_deleted = 0
                ), 0),
                last_received_at = COALESCE((
                    SELECT MAX(datetime(m.date, 'unixepoch')) FROM messages m
                    WHERE m.from_address = newsletter_feeds.sender_address
                      AND m.account_id = newsletter_feeds.account_id
                      AND m.is_deleted = 0
                ), newsletter_feeds.last_received_at)",
        );

        created
    }

    fn get_feed_id(conn: &rusqlite::Connection, sender: &str) -> i64 {
        conn.query_row(
            "SELECT id FROM newsletter_feeds WHERE sender_address = ?1",
            rusqlite::params![sender],
            |row| row.get(0),
        )
        .unwrap()
    }

    fn get_feed_sender(conn: &rusqlite::Connection, feed_id: i64) -> (String, String) {
        conn.query_row(
            "SELECT sender_address, account_id FROM newsletter_feeds WHERE id = ?1",
            rusqlite::params![feed_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap()
    }

    fn query_feeds(conn: &rusqlite::Connection, account_id: Option<&str>) -> Vec<NewsletterFeed> {
        if let Some(acct) = account_id {
            let mut stmt = conn
                .prepare(
                    "SELECT nf.id, nf.account_id, nf.sender_address, nf.sender_name,
                            nf.display_name, nf.is_muted, nf.is_favorite, nf.last_received_at,
                            nf.article_count, nf.created_at,
                            COALESCE((
                                SELECT COUNT(*) FROM messages m
                                WHERE m.from_address = nf.sender_address
                                  AND m.account_id = nf.account_id
                                  AND m.is_read = 0
                                  AND m.is_deleted = 0
                            ), 0) as unread_count
                     FROM newsletter_feeds nf
                     WHERE nf.account_id = ?1
                     ORDER BY nf.is_favorite DESC, nf.last_received_at DESC",
                )
                .unwrap();
            stmt.query_map(rusqlite::params![acct], map_feed_row)
                .unwrap()
                .filter_map(|r| r.ok())
                .collect()
        } else {
            let mut stmt = conn
                .prepare(
                    "SELECT nf.id, nf.account_id, nf.sender_address, nf.sender_name,
                            nf.display_name, nf.is_muted, nf.is_favorite, nf.last_received_at,
                            nf.article_count, nf.created_at,
                            COALESCE((
                                SELECT COUNT(*) FROM messages m
                                WHERE m.from_address = nf.sender_address
                                  AND m.account_id = nf.account_id
                                  AND m.is_read = 0
                                  AND m.is_deleted = 0
                            ), 0) as unread_count
                     FROM newsletter_feeds nf
                     ORDER BY nf.is_favorite DESC, nf.last_received_at DESC",
                )
                .unwrap();
            stmt.query_map([], map_feed_row)
                .unwrap()
                .filter_map(|r| r.ok())
                .collect()
        }
    }

    fn map_feed_row(row: &rusqlite::Row) -> rusqlite::Result<NewsletterFeed> {
        Ok(NewsletterFeed {
            id: row.get(0)?,
            account_id: row.get(1)?,
            sender_address: row.get(2)?,
            sender_name: row.get(3)?,
            display_name: row.get(4)?,
            is_muted: row.get::<_, i64>(5)? != 0,
            is_favorite: row.get::<_, i64>(6)? != 0,
            last_received_at: row.get(7)?,
            article_count: row.get(8)?,
            created_at: row.get(9)?,
            unread_count: row.get(10)?,
        })
    }

    fn query_articles(
        conn: &rusqlite::Connection,
        feed_id: i64,
        limit: i64,
        offset: i64,
    ) -> (Vec<FeedArticle>, i64) {
        let (sender, account) = get_feed_sender(conn, feed_id);

        let mut stmt = conn
            .prepare(
                "SELECT m.id, m.message_id, m.subject, m.snippet, m.date, m.is_read, m.has_attachments
                 FROM messages m
                 WHERE m.from_address = ?1
                   AND m.account_id = ?2
                   AND m.is_deleted = 0
                 ORDER BY m.date DESC
                 LIMIT ?3 OFFSET ?4",
            )
            .unwrap();

        let articles: Vec<FeedArticle> = stmt
            .query_map(
                rusqlite::params![sender, account, limit, offset],
                |row| {
                    Ok(FeedArticle {
                        id: row.get(0)?,
                        message_id: row.get(1)?,
                        subject: row.get(2)?,
                        snippet: row.get(3)?,
                        date: row.get(4)?,
                        is_read: row.get::<_, i64>(5)? != 0,
                        has_attachments: row.get::<_, i64>(6)? != 0,
                    })
                },
            )
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        let total: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM messages
                 WHERE from_address = ?1 AND account_id = ?2 AND is_deleted = 0",
                rusqlite::params![sender, account],
                |row| row.get(0),
            )
            .unwrap_or(0);

        (articles, total)
    }
}
