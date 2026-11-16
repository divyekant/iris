//! Tests of the measures endpoints (card R5-12): `GET /api/measures`, and
//! `POST /api/measures/use` and `/open`.

use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use iris_api::Auth;
use iris_changeset::{
    add_action, approve, create, decide, submit, ActionId, Author, AuthorKind, By, State, Verb,
};
use iris_store::{NewMessage, Store};
use rusqlite::params;
use serde_json::{json, Map, Value};
use tower::ServiceExt;

/// 2026-11-16T12:00:00Z.
const NOON: i64 = 1_794_830_400_000;

fn store(name: &str) -> Arc<Store> {
    let dir = std::env::temp_dir().join(format!("iris-api-measures-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    Arc::new(Store::open(&dir).unwrap())
}

async fn call(store: &Arc<Store>, req: Request<Body>) -> (StatusCode, Value) {
    let auth = Arc::new(Auth::new("127.0.0.1:4800".parse().unwrap()).unwrap());
    let (mut parts, body) = req.into_parts();
    parts
        .headers
        .insert("host", "127.0.0.1:4800".parse().unwrap());
    parts
        .headers
        .insert("authorization", auth.bearer().parse().unwrap());
    let res = iris_api::router(store.clone(), auth)
        .oneshot(Request::from_parts(parts, body))
        .await
        .unwrap();
    let code = res.status();
    let body = to_bytes(res.into_body(), 1 << 20).await.unwrap();
    (code, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

async fn get(store: &Arc<Store>) -> Value {
    let (code, body) = call(
        store,
        Request::get("/api/measures").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    body
}

async fn post(store: &Arc<Store>, uri: &str, body: &str) -> (StatusCode, Value) {
    let req = Request::post(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    call(store, req).await
}

fn today() -> String {
    iris_api::rfc3339(now())[..10].to_string()
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn days(s: &Store) -> Vec<(String, i64)> {
    s.read(|c| {
        let mut q = c.prepare("SELECT day, opens FROM use_day ORDER BY day")?;
        let rows = q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    })
    .unwrap()
}

/// With no data, each measure is empty or zero, and the memory is there.
#[tokio::test]
async fn an_empty_store_has_empty_measures() {
    let s = store("empty");
    let m = get(&s).await;
    assert_eq!(m["days"], json!([]));
    assert_eq!(m["first_brief"], Value::Null);
    assert_eq!(m["mirror"], json!([]));
    assert_eq!(m["approvals"], json!({"decided": 0, "approved": 0}));
    assert_eq!(m["undo"], json!({"applied": 0, "undone": 0}));
    assert_eq!(m["memory"]["messages"], 0);
    assert!(m["memory"]["rss_kib"].as_i64().unwrap() > 1000, "{m}");
}

/// A use makes the day; each open adds one to it. The answer gives the
/// newest 28 days, the newest first.
#[tokio::test]
async fn use_and_open_count_on_the_day_of_the_person() {
    let s = store("count");
    let day = today();
    let body = json!({ "day": day }).to_string();
    assert_eq!(
        post(&s, "/api/measures/use", &body).await,
        (StatusCode::OK, json!({}))
    );
    assert_eq!(days(&s), vec![(day.clone(), 0)]);
    post(&s, "/api/measures/open", &body).await;
    post(&s, "/api/measures/open", &body).await;
    post(&s, "/api/measures/use", &body).await;
    assert_eq!(days(&s), vec![(day.clone(), 2)]);

    s.write(|tx| {
        for d in 1..=30 {
            tx.execute(
                "INSERT INTO use_day(day, opens) VALUES (date('2020-01-01', ?1), ?2)",
                params![format!("+{d} days"), d],
            )?;
        }
        Ok(())
    })
    .unwrap();
    let m = get(&s).await;
    let got = m["days"].as_array().unwrap();
    assert_eq!(got.len(), 28);
    assert_eq!(got[0], json!({"day": day, "opens": 2}));
    assert_eq!(got[1], json!({"day": "2020-01-31", "opens": 30}));
    assert_eq!(got[27]["day"], "2020-01-05");
}

/// The day is the date of the engine, or one day before or after it. Any
/// other text is a 400 and writes nothing.
#[test]
fn only_a_date_near_today_counts() {
    let s = store("days");
    let good = ["2026-11-15", "2026-11-16", "2026-11-17"];
    for d in good {
        iris_api::measures::count_use(&s, d, true, NOON).unwrap();
    }
    let mut bad: Vec<String> = [
        "",
        "2026-11-14",
        "2026-11-18",
        "2026-02-30",
        "2026-11-1",
        "2026-11-016",
        "20261116",
        "2026/11/16",
        "2026-11-16 ",
        " 2026-11-16",
        "2026-11-16T00:00",
        "+2026-11-16",
        "２０２６-11-16",
        "2026-13-01",
        "2026-00-10",
        "2026-11-00",
        "now",
        "2026-11-16'",
        "1970-01-01",
    ]
    .iter()
    .map(|d| d.to_string())
    .collect();
    // Each change of one character of a good day that is not again a good day.
    for d in good {
        for i in 0..d.len() {
            for c in ['0', '9', '-', 'a', ' ', '/'] {
                let mut t = d.as_bytes().to_vec();
                t[i] = c as u8;
                let t = String::from_utf8(t).unwrap();
                if !good.contains(&t.as_str()) {
                    bad.push(t);
                }
            }
        }
    }
    for d in &bad {
        let e = iris_api::measures::count_use(&s, d, true, NOON);
        assert!(matches!(e, Err(iris_api::Error::Query(_))), "{d:?}: {e:?}");
    }
    assert_eq!(
        days(&s),
        good.iter().map(|d| (d.to_string(), 1)).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn a_bad_request_writes_nothing() {
    let s = store("bad");
    let day = today();
    for (uri, body, code) in [
        (
            "/api/measures/view",
            json!({"day": day}).to_string(),
            StatusCode::NOT_FOUND,
        ),
        ("/api/measures/open", "{}".into(), StatusCode::BAD_REQUEST),
        (
            "/api/measures/open",
            json!({"day": day, "n": 2}).to_string(),
            StatusCode::BAD_REQUEST,
        ),
        (
            "/api/measures/open",
            json!({"day": 20261116}).to_string(),
            StatusCode::BAD_REQUEST,
        ),
        (
            "/api/measures/open",
            json!({"day": "2001-01-01"}).to_string(),
            StatusCode::BAD_REQUEST,
        ),
    ] {
        let (got, _) = post(&s, uri, &body).await;
        assert_eq!(got, code, "{uri} {body}");
    }
    assert_eq!(days(&s), vec![]);
}

fn times(s: &Store) -> Vec<(String, i64)> {
    s.read(|c| {
        let mut q = c.prepare("SELECT name, at FROM measure_time ORDER BY name")?;
        let rows = q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    })
    .unwrap()
}

fn add_item(s: &Store, account: i64) {
    s.write(|tx| {
        tx.execute(
            "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
             VALUES ('feed', ?1, 'new', '{}', 0, 0)",
            [account],
        )?;
        Ok(())
    })
    .unwrap();
}

/// The first brief runs from the first account (the connect step) to the
/// first item. Each time is written once, at the event, and never moves:
/// not with more accounts or items, not when the items are made again.
#[tokio::test]
async fn first_brief_times_are_written_once_at_the_event() {
    let s = store("brief");
    assert_eq!(get(&s).await["first_brief"], Value::Null);
    let before = now();
    let a = s.add_account("gmail", "a@example.com").unwrap();
    let after = now();
    let t = times(&s);
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].0, "connect");
    assert!((before..=after).contains(&t[0].1), "{t:?}");
    // No item yet: no brief yet, also while the sync and the model run.
    assert_eq!(get(&s).await["first_brief"], Value::Null);

    add_item(&s, a);
    let first = times(&s);
    // In the order of the names: brief, then connect.
    assert_eq!(first.len(), 2);
    assert_eq!(first[1], t[0]);
    assert!(first[0].1 >= first[1].1);
    s.add_account("gmail", "b@example.com").unwrap();
    add_item(&s, a);
    s.write(|tx| {
        tx.execute("DELETE FROM item", [])?;
        Ok(())
    })
    .unwrap();
    add_item(&s, a);
    assert_eq!(times(&s), first);
    let e = s.write(|tx| {
        tx.execute("UPDATE measure_time SET at = 0", [])?;
        Ok(())
    });
    assert!(e.is_err());
    assert_eq!(times(&s), first);

    let m = get(&s).await;
    assert_eq!(m["first_brief"]["connect"], iris_api::rfc3339(first[0].1));
    assert_eq!(m["first_brief"]["brief"], iris_api::rfc3339(first[1].1));
    assert_eq!(
        m["first_brief"]["seconds"],
        (first[1].1 - first[0].1) / 1000
    );
}

/// A store from before migration 12 gets the times from the oldest sync
/// and the oldest item.
#[test]
fn migration_12_takes_the_times_of_an_older_store() {
    let dir = std::env::temp_dir().join(format!("iris-api-measures-old-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    {
        let s = Store::open(&dir).unwrap();
        let a = s.add_account("gmail", "a@example.com").unwrap();
        s.write(|tx| {
            tx.execute_batch(
                "DROP TRIGGER measure_connect; DROP TRIGGER measure_brief;
                 DROP TABLE use_day; DROP TABLE measure_time; PRAGMA user_version = 11;",
            )?;
            tx.execute(
                "INSERT INTO sync_log(account_id, kind, started_at) VALUES (?1, 'first', ?2), (?1, 'incremental', ?3)",
                params![a, NOON + 5000, NOON],
            )?;
            tx.execute(
                "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
                 VALUES ('feed', ?1, 'new', '{}', ?2, 0), ('feed', ?1, 'new', '{}', ?3, 0)",
                params![a, NOON + 400_000, NOON + 340_000],
            )?;
            Ok(())
        })
        .unwrap();
    }
    let s = Store::open(&dir).unwrap();
    assert_eq!(
        times(&s),
        vec![("brief".into(), NOON + 340_000), ("connect".into(), NOON)]
    );
    let m = iris_api::measures::measures(&s, None).unwrap();
    assert_eq!(m.first_brief.unwrap().seconds, 340);
}

/// The mirror check: the newest verify row of each account.
#[tokio::test]
async fn mirror_gives_the_newest_verify_of_each_account() {
    let s = store("mirror");
    let a = s.add_account("gmail", "a@example.com").unwrap();
    let b = s.add_account("gmail", "b@example.com").unwrap();
    s.add_account("gmail", "c@example.com").unwrap();
    s.write(|tx| {
        let mut q = tx.prepare(
            "INSERT INTO sync_log(account_id, kind, started_at, ended_at, result)
             VALUES (?1, ?2, ?3, ?3, ?4)",
        )?;
        q.execute(params![
            a,
            "verify",
            NOON,
            "4 differences, 0 skipped\nlabel x: other name"
        ])?;
        q.execute(params![
            a,
            "verify",
            NOON + 1000,
            "0 differences, 2 skipped"
        ])?;
        q.execute(params![a, "incremental", NOON + 2000, "ok"])?;
        q.execute(params![
            b,
            "verify",
            NOON,
            "1 differences, 0 skipped, then error: HTTP 401"
        ])?;
        Ok(())
    })
    .unwrap();
    let m = get(&s).await;
    assert_eq!(
        m["mirror"],
        json!([
            {"account": "a@example.com", "checked": "2026-11-16T12:00:01Z", "differences": 0, "complete": true},
            {"account": "b@example.com", "checked": "2026-11-16T12:00:00Z", "differences": 1, "complete": false},
        ])
    );
}

fn agent() -> Author {
    Author {
        kind: AuthorKind::Agent,
        name: Some("Claude Code".into()),
        session: None,
        job: None,
    }
}

/// Approvals count the decisions of the person on the actions of another
/// author. The state verbs of the person are not proposals. Undo counts
/// the applied actions that an undo reversed, and not the undo itself.
#[tokio::test]
async fn approvals_and_undo_count_the_decisions_of_the_person() {
    let s = store("decide");
    let a = s.add_account("gmail", "owner@example.com").unwrap();
    let m = |id: &str| NewMessage {
        provider_id: id.into(),
        thread_provider_id: id.into(),
        ..Default::default()
    };
    s.put_messages(a, &[m("g1"), m("g2"), m("g3")]).unwrap();
    let cs = create(&s, &agent(), NOON).unwrap();
    for t in ["m-1", "m-2", "m-3"] {
        add_action(
            &s,
            cs,
            &agent(),
            Verb::Archive,
            &[t],
            &Map::new(),
            Some("Old."),
        )
        .unwrap();
    }
    submit(&s, cs, &agent(), "Old mail.", NOON).unwrap();
    let id = |n| ActionId {
        changeset: cs,
        revision: 1,
        n,
    };
    let hash = |n: i64| -> String {
        s.read(|c| {
            Ok(c.query_row(
                "SELECT hash FROM changeset_action WHERE changeset_id = ?1 AND n = ?2",
                params![cs, n],
                |r| r.get(0),
            )?)
        })
        .unwrap()
    };
    approve(&s, id(1), &hash(1), NOON).unwrap();
    approve(&s, id(2), &hash(2), NOON).unwrap();
    decide(&s, id(3), State::Denied, By::Person, Some("No."), NOON).unwrap();

    // Two state verbs of the person: one applied, one failed (the second
    // close of a done commitment).
    let mut doc = iris_spec::read_json(
        &iris_spec::schema_dir()
            .join("examples")
            .join("commitment-you-owe.json"),
    );
    doc["id"] = json!("c-1");
    doc["status"] = json!("open");
    s.write(|tx| {
        tx.execute(
            "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
             VALUES ('commitment', ?1, 'open', ?2, 0, 0)",
            params![a, doc.to_string()],
        )?;
        tx.execute(
            "INSERT INTO item_source(item_id, message_id, role) VALUES (1, 1, 'origin')",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    assert_eq!(
        post(&s, "/api/items/c-1/close", "{}").await.0,
        StatusCode::OK
    );
    assert_eq!(
        post(&s, "/api/items/c-1/close", "{}").await.0,
        StatusCode::CONFLICT
    );

    let m = get(&s).await;
    assert_eq!(m["approvals"], json!({"decided": 3, "approved": 2}));
    assert_eq!(m["undo"], json!({"applied": 1, "undone": 0}));
    assert_eq!(m["memory"]["messages"], 3);

    // No undo exists yet (R2-03): make the failed entry the undo of the
    // applied one, past the triggers.
    s.write(|tx| {
        tx.execute_batch(
            "DROP TRIGGER ledger_change; DROP TRIGGER ledger_state;
             UPDATE ledger SET state = 'applied' WHERE id = 2;
             UPDATE ledger SET state = 'undone', undone_by = 2 WHERE id = 1;",
        )?;
        Ok(())
    })
    .unwrap();
    let m = get(&s).await;
    assert_eq!(m["undo"], json!({"applied": 1, "undone": 1}));
}
