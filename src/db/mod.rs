pub mod migrations;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;
use rusqlite::Connection;
use std::path::Path;

pub type DbPool = Pool<SqliteConnectionManager>;

pub fn create_pool(database_url: &str) -> Result<DbPool, Box<dyn std::error::Error>> {
    if let Some(parent) = Path::new(database_url).parent() {
        std::fs::create_dir_all(parent)?;
    }
    let manager = SqliteConnectionManager::file(database_url);
    let pool = Pool::builder().max_size(10).build(manager)?;
    let conn = pool.get()?;
    configure_connection(&conn)?;
    Ok(pool)
}

fn configure_connection(conn: &Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = NORMAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;",
    )?;
    Ok(())
}

#[cfg(test)]
pub fn create_test_pool() -> DbPool {
    let manager = SqliteConnectionManager::memory();
    let pool = Pool::builder().max_size(1).build(manager).unwrap();
    let conn = pool.get().unwrap();
    configure_connection(&conn).unwrap();
    migrations::run(&conn).unwrap();
    pool
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_pool_and_run_migrations() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        assert!(tables.contains(&"accounts".to_string()));
        assert!(tables.contains(&"messages".to_string()));
        assert!(tables.contains(&"config".to_string()));
        assert!(tables.contains(&"schema_version".to_string()));
    }

    #[test]
    fn test_default_theme_is_system() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let theme: String = conn
            .query_row("SELECT value FROM config WHERE key = 'theme'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(theme, "system");
    }

    #[test]
    fn test_fts5_index_exists() {
        let pool = create_test_pool();
        let conn = pool.get().unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='fts_messages'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(count, 1);
    }
}
