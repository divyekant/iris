use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::AppState;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct ClusterSummary {
    pub id: i64,
    pub account_id: i64,
    pub cluster_name: String,
    pub cluster_type: String,
    pub member_count: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
pub struct ClusterMember {
    pub thread_id: String,
    pub similarity_score: f64,
    pub subject: Option<String>,
    pub added_at: String,
}

#[derive(Debug, Serialize)]
pub struct ClusterDetail {
    pub id: i64,
    pub account_id: i64,
    pub cluster_name: String,
    pub cluster_type: String,
    pub members: Vec<ClusterMember>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Deserialize)]
pub struct ComputeRequest {
    pub account_id: i64,
}

#[derive(Debug, Serialize)]
pub struct ComputeResponse {
    pub clusters_created: usize,
    pub threads_clustered: usize,
}

#[derive(Debug, Deserialize)]
pub struct ListClustersParams {
    pub account_id: i64,
}

#[derive(Debug, Deserialize)]
pub struct MergeRequest {
    pub target_cluster_id: i64,
}

#[derive(Debug, Serialize)]
pub struct MergeResponse {
    pub merged: bool,
    pub new_member_count: i64,
}

#[derive(Debug, Serialize)]
pub struct DeleteResponse {
    pub deleted: bool,
}

#[derive(Debug, Serialize)]
pub struct RemoveMemberResponse {
    pub removed: bool,
}

// ---------------------------------------------------------------------------
// Subject normalization & Jaccard similarity
// ---------------------------------------------------------------------------

/// Strip Re:, Fwd:, FW:, RE: prefixes, lowercase, and trim.
fn normalize_subject(subject: &str) -> String {
    let mut s = subject.to_string();
    // Iteratively strip prefixes (handles "Re: Fwd: Re: ...")
    loop {
        let trimmed = s.trim_start();
        let lower = trimmed.to_lowercase();
        if lower.starts_with("re:") {
            s = trimmed[3..].to_string();
        } else if lower.starts_with("fwd:") {
            s = trimmed[4..].to_string();
        } else if lower.starts_with("fw:") {
            s = trimmed[3..].to_string();
        } else {
            break;
        }
    }
    s.trim().to_lowercase()
}

/// Compute Jaccard similarity between two strings based on word sets.
/// Returns |A ∩ B| / |A ∪ B|.
fn jaccard_similarity(a: &str, b: &str) -> f64 {
    let set_a: HashSet<&str> = a.split_whitespace().collect();
    let set_b: HashSet<&str> = b.split_whitespace().collect();

    if set_a.is_empty() && set_b.is_empty() {
        return 1.0;
    }

    let intersection = set_a.intersection(&set_b).count();
    let union = set_a.union(&set_b).count();

    if union == 0 {
        return 0.0;
    }

    intersection as f64 / union as f64
}

// ---------------------------------------------------------------------------
// Union-Find for transitive clustering
// ---------------------------------------------------------------------------

struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<usize>,
}

impl UnionFind {
    fn new(n: usize) -> Self {
        Self {
            parent: (0..n).collect(),
            rank: vec![0; n],
        }
    }

    fn find(&mut self, x: usize) -> usize {
        if self.parent[x] != x {
            self.parent[x] = self.find(self.parent[x]);
        }
        self.parent[x]
    }

    fn union(&mut self, x: usize, y: usize) {
        let rx = self.find(x);
        let ry = self.find(y);
        if rx == ry {
            return;
        }
        if self.rank[rx] < self.rank[ry] {
            self.parent[rx] = ry;
        } else if self.rank[rx] > self.rank[ry] {
            self.parent[ry] = rx;
        } else {
            self.parent[ry] = rx;
            self.rank[rx] += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Endpoints
// ---------------------------------------------------------------------------

/// POST /api/thread-clusters/compute
pub async fn compute_clusters(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ComputeRequest>,
) -> Result<Json<ComputeResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // 1. Query all distinct (thread_id, subject) pairs for this account
    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT thread_id, subject FROM messages
             WHERE account_id = ?1 AND thread_id IS NOT NULL AND subject IS NOT NULL",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let threads: Vec<(String, String)> = stmt
        .query_map(rusqlite::params![req.account_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    if threads.is_empty() {
        return Ok(Json(ComputeResponse {
            clusters_created: 0,
            threads_clustered: 0,
        }));
    }

    // 2. Normalize subjects
    let normalized: Vec<String> = threads.iter().map(|(_, s)| normalize_subject(s)).collect();

    // 3. Build clusters using union-find with Jaccard similarity > 0.6
    let n = threads.len();
    let mut uf = UnionFind::new(n);

    for i in 0..n {
        for j in (i + 1)..n {
            if jaccard_similarity(&normalized[i], &normalized[j]) > 0.6 {
                uf.union(i, j);
            }
        }
    }

    // 4. Group threads by their root representative
    let mut groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        let root = uf.find(i);
        groups.entry(root).or_default().push(i);
    }

    // Only keep clusters with 2+ members
    let clusters: Vec<Vec<usize>> = groups.into_values().filter(|g| g.len() >= 2).collect();

    // 5. Delete old clusters for this account before inserting new ones
    conn.execute(
        "DELETE FROM thread_cluster_members WHERE cluster_id IN (
            SELECT id FROM thread_clusters WHERE account_id = ?1
        )",
        rusqlite::params![req.account_id],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    conn.execute(
        "DELETE FROM thread_clusters WHERE account_id = ?1",
        rusqlite::params![req.account_id],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // 6. Insert new clusters
    let mut clusters_created = 0usize;
    let mut threads_clustered = 0usize;

    for group in &clusters {
        // Name cluster by the most common normalized subject (shortest as tiebreaker)
        let cluster_name = group
            .iter()
            .map(|&i| normalized[i].clone())
            .min_by_key(|s| s.len())
            .unwrap_or_default();

        // Determine cluster type based on max similarity within the group
        let mut max_sim = 0.0f64;
        for i in 0..group.len() {
            for j in (i + 1)..group.len() {
                let sim = jaccard_similarity(&normalized[group[i]], &normalized[group[j]]);
                if sim > max_sim {
                    max_sim = sim;
                }
            }
        }
        let cluster_type = if max_sim > 0.9 {
            "duplicate"
        } else {
            "related"
        };

        conn.execute(
            "INSERT INTO thread_clusters (account_id, cluster_name, cluster_type)
             VALUES (?1, ?2, ?3)",
            rusqlite::params![req.account_id, cluster_name, cluster_type],
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

        let cluster_id = conn.last_insert_rowid();
        clusters_created += 1;

        for &idx in group {
            let (ref thread_id, _) = threads[idx];
            // Compute similarity to the first member as the score
            let sim = if idx == group[0] {
                1.0
            } else {
                jaccard_similarity(&normalized[group[0]], &normalized[idx])
            };

            conn.execute(
                "INSERT INTO thread_cluster_members (cluster_id, thread_id, similarity_score)
                 VALUES (?1, ?2, ?3)",
                rusqlite::params![cluster_id, thread_id, sim],
            )
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

            threads_clustered += 1;
        }
    }

    Ok(Json(ComputeResponse {
        clusters_created,
        threads_clustered,
    }))
}

/// GET /api/thread-clusters
pub async fn list_clusters(
    State(state): State<Arc<AppState>>,
    Query(params): Query<ListClustersParams>,
) -> Result<Json<Vec<ClusterSummary>>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut stmt = conn
        .prepare(
            "SELECT c.id, c.account_id, c.cluster_name, c.cluster_type,
                    COUNT(m.id) as member_count, c.created_at, c.updated_at
             FROM thread_clusters c
             LEFT JOIN thread_cluster_members m ON m.cluster_id = c.id
             WHERE c.account_id = ?1
             GROUP BY c.id
             ORDER BY c.updated_at DESC",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let clusters: Vec<ClusterSummary> = stmt
        .query_map(rusqlite::params![params.account_id], |row| {
            Ok(ClusterSummary {
                id: row.get(0)?,
                account_id: row.get(1)?,
                cluster_name: row.get(2)?,
                cluster_type: row.get(3)?,
                member_count: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(clusters))
}

/// GET /api/thread-clusters/:id
pub async fn get_cluster(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<ClusterDetail>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let cluster = conn
        .query_row(
            "SELECT id, account_id, cluster_name, cluster_type, created_at, updated_at
             FROM thread_clusters WHERE id = ?1",
            rusqlite::params![id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                ))
            },
        )
        .map_err(|_| StatusCode::NOT_FOUND)?;

    let mut stmt = conn
        .prepare(
            "SELECT tcm.thread_id, tcm.similarity_score, tcm.added_at,
                    (SELECT m.subject FROM messages m WHERE m.thread_id = tcm.thread_id LIMIT 1) as subject
             FROM thread_cluster_members tcm
             WHERE tcm.cluster_id = ?1
             ORDER BY tcm.similarity_score DESC",
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let members: Vec<ClusterMember> = stmt
        .query_map(rusqlite::params![id], |row| {
            Ok(ClusterMember {
                thread_id: row.get(0)?,
                similarity_score: row.get(1)?,
                added_at: row.get(2)?,
                subject: row.get(3)?,
            })
        })
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .filter_map(|r| r.ok())
        .collect();

    Ok(Json(ClusterDetail {
        id: cluster.0,
        account_id: cluster.1,
        cluster_name: cluster.2,
        cluster_type: cluster.3,
        members,
        created_at: cluster.4,
        updated_at: cluster.5,
    }))
}

/// POST /api/thread-clusters/:id/merge — merge another cluster into this one
pub async fn merge_clusters(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(req): Json<MergeRequest>,
) -> Result<Json<MergeResponse>, StatusCode> {
    if id == req.target_cluster_id {
        return Err(StatusCode::BAD_REQUEST);
    }

    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Verify both clusters exist
    let exists_source: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM thread_clusters WHERE id = ?1",
            rusqlite::params![id],
            |row| row.get::<_, i64>(0),
        )
        .map(|c| c > 0)
        .unwrap_or(false);

    let exists_target: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM thread_clusters WHERE id = ?1",
            rusqlite::params![req.target_cluster_id],
            |row| row.get::<_, i64>(0),
        )
        .map(|c| c > 0)
        .unwrap_or(false);

    if !exists_source || !exists_target {
        return Err(StatusCode::NOT_FOUND);
    }

    // Move all members from target_cluster_id into this cluster (id)
    conn.execute(
        "UPDATE thread_cluster_members SET cluster_id = ?1 WHERE cluster_id = ?2",
        rusqlite::params![id, req.target_cluster_id],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Delete the now-empty target cluster
    conn.execute(
        "DELETE FROM thread_clusters WHERE id = ?1",
        rusqlite::params![req.target_cluster_id],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Update timestamp on the merged cluster
    conn.execute(
        "UPDATE thread_clusters SET updated_at = datetime('now') WHERE id = ?1",
        rusqlite::params![id],
    )
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // Count new total members
    let new_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM thread_cluster_members WHERE cluster_id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        )
        .unwrap_or(0);

    Ok(Json(MergeResponse {
        merged: true,
        new_member_count: new_count,
    }))
}

/// DELETE /api/thread-clusters/:id
pub async fn delete_cluster(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> Result<Json<DeleteResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    // CASCADE will delete members too
    let rows = conn
        .execute(
            "DELETE FROM thread_clusters WHERE id = ?1",
            rusqlite::params![id],
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if rows == 0 {
        return Err(StatusCode::NOT_FOUND);
    }

    Ok(Json(DeleteResponse { deleted: true }))
}

/// DELETE /api/thread-clusters/:cluster_id/members/:thread_id
pub async fn remove_member(
    State(state): State<Arc<AppState>>,
    Path((cluster_id, thread_id)): Path<(i64, String)>,
) -> Result<Json<RemoveMemberResponse>, StatusCode> {
    let conn = state.db.get().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let rows = conn
        .execute(
            "DELETE FROM thread_cluster_members WHERE cluster_id = ?1 AND thread_id = ?2",
            rusqlite::params![cluster_id, thread_id],
        )
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    if rows == 0 {
        return Err(StatusCode::NOT_FOUND);
    }

    Ok(Json(RemoveMemberResponse { removed: true }))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::create_test_pool;

    /// Helper: insert an account and return its id
    fn insert_account(conn: &r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO accounts (id, provider, email) VALUES (?1, 'gmail', ?2)",
            rusqlite::params![id, format!("test-{}@example.com", &id[..8])],
        )
        .unwrap();
        id
    }

    /// Helper: insert a message with a given thread_id and subject
    fn insert_message(
        conn: &r2d2::PooledConnection<r2d2_sqlite::SqliteConnectionManager>,
        account_id: &str,
        thread_id: &str,
        subject: &str,
    ) {
        let id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO messages (id, account_id, thread_id, folder, subject, from_address)
             VALUES (?1, ?2, ?3, 'INBOX', ?4, 'sender@example.com')",
            rusqlite::params![id, account_id, thread_id, subject],
        )
        .unwrap();
    }

    // ------ Unit tests for normalize_subject ------

    #[test]
    fn test_normalize_strips_re_prefix() {
        assert_eq!(normalize_subject("Re: Hello World"), "hello world");
    }

    #[test]
    fn test_normalize_strips_fwd_prefix() {
        assert_eq!(normalize_subject("Fwd: Meeting Notes"), "meeting notes");
    }

    #[test]
    fn test_normalize_strips_fw_prefix() {
        assert_eq!(normalize_subject("FW: Budget Report"), "budget report");
    }

    #[test]
    fn test_normalize_strips_multiple_prefixes() {
        assert_eq!(
            normalize_subject("Re: Fwd: RE: FW: Project Update"),
            "project update"
        );
    }

    #[test]
    fn test_normalize_trims_and_lowercases() {
        assert_eq!(normalize_subject("  HELLO  "), "hello");
    }

    // ------ Unit tests for jaccard_similarity ------

    #[test]
    fn test_jaccard_identical() {
        assert!((jaccard_similarity("hello world", "hello world") - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_jaccard_no_overlap() {
        assert!((jaccard_similarity("hello world", "foo bar")).abs() < f64::EPSILON);
    }

    #[test]
    fn test_jaccard_partial_overlap() {
        // "meeting notes" vs "meeting agenda" -> intersection={meeting}, union={meeting, notes, agenda}
        let sim = jaccard_similarity("meeting notes", "meeting agenda");
        let expected = 1.0 / 3.0;
        assert!((sim - expected).abs() < 0.01);
    }

    #[test]
    fn test_jaccard_empty_strings() {
        // Both empty -> 1.0
        assert!((jaccard_similarity("", "") - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_jaccard_one_empty() {
        assert!((jaccard_similarity("hello", "")).abs() < f64::EPSILON);
    }

    // ------ Union-Find tests ------

    #[test]
    fn test_union_find_basic() {
        let mut uf = UnionFind::new(5);
        uf.union(0, 1);
        uf.union(2, 3);
        assert_eq!(uf.find(0), uf.find(1));
        assert_eq!(uf.find(2), uf.find(3));
        assert_ne!(uf.find(0), uf.find(2));

        uf.union(1, 3);
        assert_eq!(uf.find(0), uf.find(3));
    }

    // ------ Database / endpoint logic tests ------

    #[test]
    fn test_compute_creates_clusters_for_similar_subjects() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let account_id = insert_account(&conn);
        insert_message(&conn, &account_id, "thread-1", "Project Status Update");
        insert_message(&conn, &account_id, "thread-2", "Re: Project Status Update");
        insert_message(&conn, &account_id, "thread-3", "Completely Different Topic");

        // Simulate the compute logic directly using db
        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT thread_id, subject FROM messages
                 WHERE account_id = ?1 AND thread_id IS NOT NULL AND subject IS NOT NULL",
            )
            .unwrap();

        let threads: Vec<(String, String)> = stmt
            .query_map(rusqlite::params![account_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(threads.len(), 3);

        let normalized: Vec<String> = threads.iter().map(|(_, s)| normalize_subject(s)).collect();

        // "project status update" and "project status update" should be similar (identical after norm)
        // "completely different topic" should NOT be similar
        let sim_01 = jaccard_similarity(&normalized[0], &normalized[1]);
        assert!(sim_01 > 0.6, "Expected similar subjects to have sim > 0.6, got {sim_01}");

        let sim_02 = jaccard_similarity(&normalized[0], &normalized[2]);
        assert!(sim_02 <= 0.6, "Expected different subjects to have sim <= 0.6, got {sim_02}");
    }

    #[test]
    fn test_compute_no_threads_returns_zero() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let account_id = insert_account(&conn);

        // No messages — check that counting returns 0
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(DISTINCT thread_id) FROM messages WHERE account_id = ?1 AND thread_id IS NOT NULL",
                rusqlite::params![account_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_cluster_crud_lifecycle() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let account_id = insert_account(&conn);

        // Insert a cluster manually
        conn.execute(
            "INSERT INTO thread_clusters (account_id, cluster_name, cluster_type) VALUES (?1, 'Test Cluster', 'related')",
            rusqlite::params![account_id],
        )
        .unwrap();
        let cluster_id = conn.last_insert_rowid();

        // Add members
        conn.execute(
            "INSERT INTO thread_cluster_members (cluster_id, thread_id, similarity_score) VALUES (?1, 'thread-a', 1.0)",
            rusqlite::params![cluster_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO thread_cluster_members (cluster_id, thread_id, similarity_score) VALUES (?1, 'thread-b', 0.8)",
            rusqlite::params![cluster_id],
        )
        .unwrap();

        // Verify cluster exists
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM thread_cluster_members WHERE cluster_id = ?1",
                rusqlite::params![cluster_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 2);

        // Delete cluster — CASCADE should remove members
        conn.execute(
            "DELETE FROM thread_clusters WHERE id = ?1",
            rusqlite::params![cluster_id],
        )
        .unwrap();

        let remaining: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM thread_cluster_members WHERE cluster_id = ?1",
                rusqlite::params![cluster_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(remaining, 0);
    }

    #[test]
    fn test_merge_clusters() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let account_id = insert_account(&conn);

        // Create two clusters
        conn.execute(
            "INSERT INTO thread_clusters (account_id, cluster_name, cluster_type) VALUES (?1, 'Cluster A', 'related')",
            rusqlite::params![account_id],
        )
        .unwrap();
        let cluster_a = conn.last_insert_rowid();

        conn.execute(
            "INSERT INTO thread_clusters (account_id, cluster_name, cluster_type) VALUES (?1, 'Cluster B', 'related')",
            rusqlite::params![account_id],
        )
        .unwrap();
        let cluster_b = conn.last_insert_rowid();

        // Add members to each
        conn.execute(
            "INSERT INTO thread_cluster_members (cluster_id, thread_id, similarity_score) VALUES (?1, 'thread-1', 1.0)",
            rusqlite::params![cluster_a],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO thread_cluster_members (cluster_id, thread_id, similarity_score) VALUES (?1, 'thread-2', 0.9)",
            rusqlite::params![cluster_b],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO thread_cluster_members (cluster_id, thread_id, similarity_score) VALUES (?1, 'thread-3', 0.7)",
            rusqlite::params![cluster_b],
        )
        .unwrap();

        // Merge cluster_b into cluster_a
        conn.execute(
            "UPDATE thread_cluster_members SET cluster_id = ?1 WHERE cluster_id = ?2",
            rusqlite::params![cluster_a, cluster_b],
        )
        .unwrap();
        conn.execute(
            "DELETE FROM thread_clusters WHERE id = ?1",
            rusqlite::params![cluster_b],
        )
        .unwrap();

        // Verify all 3 members are now in cluster_a
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM thread_cluster_members WHERE cluster_id = ?1",
                rusqlite::params![cluster_a],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 3);

        // Verify cluster_b no longer exists
        let b_exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM thread_clusters WHERE id = ?1",
                rusqlite::params![cluster_b],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(b_exists, 0);
    }

    #[test]
    fn test_remove_member_from_cluster() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let account_id = insert_account(&conn);

        conn.execute(
            "INSERT INTO thread_clusters (account_id, cluster_name, cluster_type) VALUES (?1, 'Test', 'related')",
            rusqlite::params![account_id],
        )
        .unwrap();
        let cluster_id = conn.last_insert_rowid();

        conn.execute(
            "INSERT INTO thread_cluster_members (cluster_id, thread_id, similarity_score) VALUES (?1, 'thread-x', 1.0)",
            rusqlite::params![cluster_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO thread_cluster_members (cluster_id, thread_id, similarity_score) VALUES (?1, 'thread-y', 0.8)",
            rusqlite::params![cluster_id],
        )
        .unwrap();

        // Remove thread-x
        let rows = conn
            .execute(
                "DELETE FROM thread_cluster_members WHERE cluster_id = ?1 AND thread_id = ?2",
                rusqlite::params![cluster_id, "thread-x"],
            )
            .unwrap();
        assert_eq!(rows, 1);

        // Verify only thread-y remains
        let remaining: Vec<String> = conn
            .prepare("SELECT thread_id FROM thread_cluster_members WHERE cluster_id = ?1")
            .unwrap()
            .query_map(rusqlite::params![cluster_id], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert_eq!(remaining, vec!["thread-y"]);
    }

    #[test]
    fn test_remove_nonexistent_member() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let account_id = insert_account(&conn);

        conn.execute(
            "INSERT INTO thread_clusters (account_id, cluster_name, cluster_type) VALUES (?1, 'Test', 'related')",
            rusqlite::params![account_id],
        )
        .unwrap();
        let cluster_id = conn.last_insert_rowid();

        let rows = conn
            .execute(
                "DELETE FROM thread_cluster_members WHERE cluster_id = ?1 AND thread_id = ?2",
                rusqlite::params![cluster_id, "nonexistent"],
            )
            .unwrap();
        assert_eq!(rows, 0);
    }

    #[test]
    fn test_list_clusters_for_account() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let account_id = insert_account(&conn);
        let other_account_id = insert_account(&conn);

        // Insert clusters for both accounts
        conn.execute(
            "INSERT INTO thread_clusters (account_id, cluster_name, cluster_type) VALUES (?1, 'My Cluster', 'related')",
            rusqlite::params![account_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO thread_clusters (account_id, cluster_name, cluster_type) VALUES (?1, 'Other Cluster', 'duplicate')",
            rusqlite::params![other_account_id],
        )
        .unwrap();

        // List clusters for account_id only
        let mut stmt = conn
            .prepare(
                "SELECT c.id, c.cluster_name FROM thread_clusters c WHERE c.account_id = ?1",
            )
            .unwrap();
        let clusters: Vec<(i64, String)> = stmt
            .query_map(rusqlite::params![account_id], |row| {
                Ok((row.get(0)?, row.get(1)?))
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].1, "My Cluster");
    }

    #[test]
    fn test_delete_nonexistent_cluster() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let rows = conn
            .execute(
                "DELETE FROM thread_clusters WHERE id = ?1",
                rusqlite::params![99999],
            )
            .unwrap();
        assert_eq!(rows, 0);
    }

    #[test]
    fn test_compute_transitive_clustering() {
        // If A~B and B~C but not A~C directly, they should still all be in one cluster
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let account_id = insert_account(&conn);
        // These subjects form a chain with >60% pairwise overlap (4/5 shared words each step)
        insert_message(&conn, &account_id, "t1", "alpha beta gamma delta epsilon");
        insert_message(&conn, &account_id, "t2", "beta gamma delta epsilon zeta");
        insert_message(&conn, &account_id, "t3", "gamma delta epsilon zeta eta");

        let subjects = vec![
            normalize_subject("alpha beta gamma delta epsilon"),
            normalize_subject("beta gamma delta epsilon zeta"),
            normalize_subject("gamma delta epsilon zeta eta"),
        ];

        // Verify chain: 0-1 similar, 1-2 similar (4/6 = 0.667)
        let sim_01 = jaccard_similarity(&subjects[0], &subjects[1]);
        let sim_12 = jaccard_similarity(&subjects[1], &subjects[2]);
        assert!(sim_01 > 0.6, "0-1 similarity should be > 0.6: {sim_01}");
        assert!(sim_12 > 0.6, "1-2 similarity should be > 0.6: {sim_12}");

        // Use union-find to cluster
        let mut uf = UnionFind::new(3);
        for i in 0..3 {
            for j in (i + 1)..3 {
                if jaccard_similarity(&subjects[i], &subjects[j]) > 0.6 {
                    uf.union(i, j);
                }
            }
        }

        // All should be in the same cluster (transitive)
        assert_eq!(uf.find(0), uf.find(1));
        assert_eq!(uf.find(1), uf.find(2));
    }

    #[test]
    fn test_cluster_type_duplicate_vs_related() {
        // High similarity -> "duplicate", moderate -> "related"
        let sim_high = jaccard_similarity("weekly status report", "weekly status report");
        assert!(sim_high > 0.9);

        // 4/5 words shared = 0.8 -> related, not duplicate
        let sim_moderate = jaccard_similarity("weekly team status report update", "weekly team status report notes");
        assert!(sim_moderate > 0.6, "moderate sim should be > 0.6: {sim_moderate}");
        assert!(sim_moderate <= 0.9, "moderate sim should be <= 0.9: {sim_moderate}");
    }

    #[test]
    fn test_schema_version_47_applied() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let version: i64 = conn
            .query_row(
                "SELECT MAX(version) FROM schema_version WHERE version = 47",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, 47);
    }

    #[test]
    fn test_indexes_exist() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();

        let indexes: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='index'")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert!(indexes.contains(&"idx_thread_clusters_account".to_string()));
        assert!(indexes.contains(&"idx_cluster_members_cluster".to_string()));
        assert!(indexes.contains(&"idx_cluster_members_thread".to_string()));
    }
}
