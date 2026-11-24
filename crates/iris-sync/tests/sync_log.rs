//! Sync rows that a stopped process left with no end (issue #710).

use iris_store::Store;

fn rows(store: &Store) -> Vec<(i64, Option<i64>, Option<String>)> {
    store
        .read(|c| {
            let mut st =
                c.prepare("SELECT account_id, ended_at, result FROM sync_log ORDER BY id")?;
            let v = st
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<rusqlite::Result<_>>()?;
            Ok(v)
        })
        .unwrap()
}

#[test]
fn a_row_with_no_end_closes_only_when_no_sync_of_its_account_runs() {
    let dir = std::env::temp_dir().join(format!("iris-sync-log-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let a = store.add_account("gmail", "a@example.com").unwrap();
    let b = store.add_account("gmail", "b@example.com").unwrap();
    store
        .write(|tx| {
            tx.execute(
                "INSERT INTO sync_log(account_id, kind, started_at) VALUES (?1, 'incremental', 1), (?2, 'incremental', 1)",
                [a, b],
            )?;
            Ok(())
        })
        .unwrap();
    // A sync of `b` runs: its row stays open.
    let held = iris_sync::lock(&store, b).unwrap();
    assert_eq!(iris_sync::close_stopped(&store, a).unwrap(), 1);
    assert_eq!(iris_sync::close_stopped(&store, b).unwrap(), 0);
    let r = rows(&store);
    assert!(r[0].1.is_some());
    assert_eq!(r[0].2.as_deref(), Some(iris_sync::STOPPED));
    assert_eq!((r[1].1, r[1].2.as_deref()), (None, None));
    drop(held);
    assert_eq!(iris_sync::close_stopped(&store, b).unwrap(), 1);
    assert_eq!(
        iris_sync::close_stopped(&store, b).unwrap(),
        0,
        "a closed row stays"
    );
    assert!(rows(&store)
        .iter()
        .all(|r| r.2.as_deref() == Some(iris_sync::STOPPED)));
    let _ = std::fs::remove_dir_all(&dir);
}
