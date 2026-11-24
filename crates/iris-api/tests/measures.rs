//! Tests of the measures endpoints (card R5-12): `GET /api/measures`, and
//! the counts from the day header of each request.

use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use iris_api::Auth;
use iris_changeset::{
    add_action, approve, create, decide, revise, submit, ActionId, Author, AuthorKind, By, State,
    Verb,
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
        let mut q = c.prepare(
            "SELECT day, (SELECT count(*) FROM use_open o WHERE o.day = d.day)
             FROM use_day d ORDER BY day",
        )?;
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

/// GET with the day header of the web client.
async fn visit(store: &Arc<Store>, uri: &str, day: Option<&str>) -> StatusCode {
    let mut req = Request::get(uri);
    if let Some(day) = day {
        req = req.header(iris_api::measures::DAY, day);
    }
    call(store, req.body(Body::empty()).unwrap()).await.0
}

/// Two messages of one account, `m-1` and `m-2`.
fn two_messages(s: &Store) {
    let a = s.add_account("gmail", "a@example.com").unwrap();
    let m = |id: &str| NewMessage {
        provider_id: id.into(),
        thread_provider_id: id.into(),
        ..Default::default()
    };
    s.put_messages(a, &[m("g1"), m("g2")]).unwrap();
}

/// Each request of the web client is a use of its day, and each mail view
/// is an open of that mail. The answer gives the newest 28 days, the
/// newest first.
#[tokio::test]
async fn use_and_open_count_on_the_day_of_the_person() {
    let s = store("count");
    two_messages(&s);
    let day = today();
    // The use counts before the request runs: the first read of the
    // measures of a day already has that day.
    let req = Request::get("/api/measures")
        .header(iris_api::measures::DAY, &day)
        .body(Body::empty())
        .unwrap();
    let (code, m) = call(&s, req).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(m["days"], json!([{"day": day, "opens": 0}]));
    for uri in [
        "/api/messages/m-1",
        "/api/messages/m-2",
        "/api/messages/m-1",
    ] {
        assert_eq!(visit(&s, uri, Some(&day)).await, StatusCode::OK);
    }
    // The raw source and a message that is not there are not an open.
    visit(&s, "/api/messages/m-1/raw", Some(&day)).await;
    assert_eq!(
        visit(&s, "/api/messages/m-9", Some(&day)).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(days(&s), vec![(day.clone(), 2)]);

    s.write(|tx| {
        for d in 1..=30 {
            let day: String = tx.query_row(
                "SELECT date('2020-01-01', ?1)",
                [format!("+{d} days")],
                |r| r.get(0),
            )?;
            tx.execute("INSERT INTO use_day(day) VALUES (?1)", [&day])?;
            for m in 0..d {
                tx.execute(
                    "INSERT INTO use_open(day, message_id) VALUES (?1, ?2)",
                    params![day, m],
                )?;
            }
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

/// Invariant: the opens of a day are the distinct mails that the person
/// opened on it, whatever the count of requests. A retry of an open whose
/// answer got lost (the write is done) changes nothing.
#[test]
fn a_repeated_open_counts_once() {
    let s = store("repeat");
    let day = "2026-11-16";
    // A fixed sequence of repeats: each mail i is sent (i * 7) % 5 + 1 times,
    // in an interleaved order.
    let mut sent = Vec::new();
    for round in 0..5 {
        for i in 1..=9 {
            if round < (i * 7) % 5 + 1 {
                sent.push(format!("m-{i}"));
            }
        }
    }
    for m in &sent {
        iris_api::measures::count_use(&s, day, Some(m), NOON).unwrap();
        iris_api::measures::count_use(&s, day, None, NOON).unwrap();
    }
    assert!(sent.len() > 9);
    assert_eq!(days(&s), vec![(day.to_string(), 9)]);
}

/// The day is the date of the engine, or one day before or after it. Any
/// other text is a 400 and writes nothing.
#[test]
fn only_a_date_near_today_counts() {
    let s = store("days");
    let good = ["2026-11-15", "2026-11-16", "2026-11-17"];
    for d in good {
        iris_api::measures::count_use(&s, d, Some("m-1"), NOON).unwrap();
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
        let e = iris_api::measures::count_use(&s, d, Some("m-1"), NOON);
        assert!(matches!(e, Err(iris_api::Error::Query(_))), "{d:?}: {e:?}");
    }
    assert_eq!(
        days(&s),
        good.iter().map(|d| (d.to_string(), 1)).collect::<Vec<_>>()
    );
}

/// A request with no day header, or with a day that is not near today,
/// counts nothing and runs as usual.
#[tokio::test]
async fn a_request_with_no_good_day_counts_nothing() {
    let s = store("bad");
    two_messages(&s);
    for day in [None, Some("2001-01-01"), Some("2026-02-30"), Some("now")] {
        assert_eq!(
            visit(&s, "/api/messages/m-1", day).await,
            StatusCode::OK,
            "{day:?}"
        );
    }
    assert_eq!(days(&s), vec![]);
    // A good day after a bad one still counts.
    assert_eq!(
        visit(&s, "/api/status", Some(&today())).await,
        StatusCode::OK
    );
    assert_eq!(days(&s), vec![(today(), 0)]);
}

fn times(s: &Store) -> Vec<(String, i64)> {
    s.read(|c| {
        let mut q = c.prepare("SELECT name, at FROM measure_time ORDER BY name")?;
        let rows = q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    })
    .unwrap()
}

/// One item of the example `example` (a file of docs/spec/schema/examples),
/// from message `message`, with the changes of `set`.
fn put_item(s: &Store, account: i64, example: &str, message: i64, set: Value) {
    let mut doc = iris_spec::read_json(&iris_spec::schema_dir().join("examples").join(example));
    for (k, v) in set.as_object().unwrap() {
        doc[k] = v.clone();
    }
    s.write(|tx| {
        tx.execute(
            "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 0, 0)",
            params![doc["form"].as_str(), account, doc["status"].as_str(), doc.to_string()],
        )?;
        tx.execute(
            "INSERT INTO item_source(item_id, message_id, role) VALUES (last_insert_rowid(), ?1, 'origin')",
            [message],
        )?;
        Ok(())
    })
    .unwrap();
}

async fn brief(s: &Arc<Store>) -> Value {
    let (code, body) = call(s, Request::get("/api/brief").body(Body::empty()).unwrap()).await;
    assert_eq!(code, StatusCode::OK, "{body}");
    body
}

/// The first brief runs from the first account (the connect step) to the
/// first answer of the brief with content. An item that the brief does not
/// show (an expired code) does not end it. Each time is written once and
/// never moves: not with more accounts, items or briefs.
#[tokio::test]
async fn first_brief_runs_to_the_first_brief_with_content() {
    let s = store("brief");
    assert_eq!(get(&s).await["first_brief"], Value::Null);
    let before = now();
    let a = s.add_account("gmail", "a@example.com").unwrap();
    let after = now();
    let t = times(&s);
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].0, "connect");
    assert!((before..=after).contains(&t[0].1), "{t:?}");
    let m = |id: &str| NewMessage {
        provider_id: id.into(),
        thread_provider_id: id.into(),
        ..Default::default()
    };
    s.put_messages(a, &[m("g1"), m("g2")]).unwrap();

    // An empty brief, then a brief with only an expired code: no content.
    brief(&s).await;
    iris_api::measures::check_brief(&s, now()).unwrap();
    put_item(
        &s,
        a,
        "signal-code.json",
        1,
        json!({"id": "s-1", "expires": "2020-01-01T00:00:00Z"}),
    );
    assert_eq!(brief(&s).await["signals"]["count"], 0);
    iris_api::measures::check_brief(&s, now()).unwrap();
    assert_eq!(times(&s).len(), 1);
    assert_eq!(get(&s).await["first_brief"], Value::Null);

    // The two events are in different seconds, so a swap of the two times shows.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    put_item(
        &s,
        a,
        "feed.json",
        2,
        json!({"id": "f-2", "status": "new", "attention": {"state": "shown"}}),
    );
    // The engine finds the content with no visit of the brief.
    iris_api::measures::check_brief(&s, now()).unwrap();
    assert_eq!(times(&s).len(), 2);
    assert_eq!(brief(&s).await["feed"]["count"], 1);
    let first = times(&s);
    // In the order of the names: brief, then connect.
    assert_eq!(first.len(), 2);
    assert_eq!(first[1], t[0]);

    s.add_account("gmail", "b@example.com").unwrap();
    brief(&s).await;
    assert_eq!(times(&s), first);
    let e = s.write(|tx| {
        tx.execute("UPDATE measure_time SET at = 0", [])?;
        Ok(())
    });
    assert!(e.is_err());
    assert_eq!(times(&s), first);

    let m = get(&s).await;
    let (brief, connect) = (first[0].1, first[1].1);
    assert!(brief > connect, "{first:?}");
    assert_eq!(m["first_brief"]["connect"], iris_api::rfc3339(connect));
    assert_eq!(m["first_brief"]["brief"], iris_api::rfc3339(brief));
    // Rounded up: a time just over the target does not show as met.
    assert_eq!(m["first_brief"]["seconds"], (brief - connect + 999) / 1000);
}

/// Make the store a store of migration 26 again. The later migrations (28
/// the write view, 32 the review comments, 33 the nudges) run again after
/// 27, so what they add goes too.
fn downgrade(s: &Store) {
    s.write(|tx| {
        tx.execute_batch(
            "DROP TRIGGER measure_connect;
             DROP TRIGGER person_decision; DROP TABLE person_decision;
             DROP TABLE use_open; DROP TABLE use_day; DROP TABLE measure_time;
             DROP TABLE write_send; DROP TABLE comment_delivery;
             DROP TABLE changeset_comment; DROP TABLE nudge_try;
             ALTER TABLE reply_draft DROP COLUMN ask;
             DROP TRIGGER job_notes_end; DROP TABLE job_notes;
             DROP TRIGGER job_session_stop; DROP TRIGGER ledger_stop; DROP TABLE stop_switch;
             ALTER TABLE account DROP COLUMN imap_host;
             ALTER TABLE account DROP COLUMN imap_port;
             ALTER TABLE account DROP COLUMN imap_user;
             ALTER TABLE account DROP COLUMN categories_off_at;
             DROP TRIGGER understand_owner_label_add; DROP TRIGGER understand_owner_label_remove;
             DROP TRIGGER header_from_addr_lower_add; DROP TRIGGER header_from_addr_lower; DROP TRIGGER talk_due_sender_add; DROP TRIGGER talk_due_sender; DROP TRIGGER talk_due_sender_remove; DROP TRIGGER model_declined_end; DROP TRIGGER model_declined_end_remove; DROP TABLE model_declined; DROP TABLE talk_due; DROP TRIGGER sent_copy_keep; DROP TRIGGER sent_copy_add; DROP VIEW iris_sent_copy; DROP INDEX draft_sent_copy; DROP TRIGGER draft_or_copy_drops_items; DROP TRIGGER draft_or_copy_talk_add; DROP TRIGGER draft_or_copy_talk_remove; DROP TRIGGER draft_or_copy_back_drops_items; DROP VIEW item_droppable;
             PRAGMA user_version = 26;",
        )?;
        Ok(())
    })
    .unwrap();
}

/// Invariant: a stored brief time is always a brief with content that the
/// engine saw. A store from before migration 27 with an account has no
/// such record, so both times are not known, and a later brief or account
/// does not write a time that is not the first.
#[test]
fn migration_27_does_not_guess_the_times_of_an_older_store() {
    let dir = std::env::temp_dir().join(format!("iris-api-measures-old-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    {
        let s = Store::open(&dir).unwrap();
        let a = s.add_account("gmail", "a@example.com").unwrap();
        downgrade(&s);
        s.write(|tx| {
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
    let unknown = || {
        s.read(|c| {
            let mut q = c.prepare("SELECT name, at IS NULL FROM measure_time ORDER BY name")?;
            let rows = q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            Ok(rows.collect::<rusqlite::Result<Vec<(String, bool)>>>()?)
        })
        .unwrap()
    };
    let none = vec![("brief".to_string(), true), ("connect".to_string(), true)];
    assert_eq!(unknown(), none);
    // A later check of the brief and a new account change nothing.
    iris_api::measures::check_brief(&s, NOON + 900_000).unwrap();
    s.add_account("gmail", "b@example.com").unwrap();
    assert_eq!(unknown(), none);
    let m = iris_api::measures::measures(&s, None).unwrap();
    assert_eq!(
        serde_json::to_value(m.first_brief).unwrap(),
        json!({"connect": null, "brief": null, "seconds": null})
    );
    drop(s);

    // No account: no time.
    let _ = std::fs::remove_dir_all(&dir);
    {
        let s = Store::open(&dir).unwrap();
        downgrade(&s);
    }
    assert_eq!(times(&Store::open(&dir).unwrap()), vec![]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A store from before migration 27 gets the decisions of the person that
/// the actions still show.
#[test]
fn migration_27_takes_the_decisions_of_an_older_store() {
    let dir = std::env::temp_dir().join(format!(
        "iris-api-measures-old-decide-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    {
        let s = Store::open(&dir).unwrap();
        decided(&s);
        downgrade(&s);
    }
    let s = Store::open(&dir).unwrap();
    let m = iris_api::measures::measures(&s, None).unwrap();
    assert_eq!(m.approvals.decided, 3);
    assert_eq!(m.approvals.approved, 2);
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
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
            // Two skipped messages were not compared: not complete.
            {"account": "a@example.com", "checked": "2026-11-16T12:00:01Z", "differences": 0, "complete": false, "skipped": 2, "stopped": false},
            {"account": "b@example.com", "checked": "2026-11-16T12:00:00Z", "differences": 1, "complete": false, "skipped": 0, "stopped": true},
            {"account": "c@example.com", "checked": null, "differences": null, "complete": false, "skipped": null, "stopped": false},
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

/// An account with 3 messages, and an agent changeset with 3 actions: the
/// person approves 2 and denies 1. Gives the changeset ID.
fn decided(s: &Store) -> i64 {
    let a = s.add_account("gmail", "owner@example.com").unwrap();
    let m = |id: &str| NewMessage {
        provider_id: id.into(),
        thread_provider_id: id.into(),
        ..Default::default()
    };
    s.put_messages(a, &[m("g1"), m("g2"), m("g3")]).unwrap();
    let cs = create(s, &agent(), NOON).unwrap();
    for t in ["m-1", "m-2", "m-3"] {
        add_action(
            s,
            cs,
            &agent(),
            Verb::Archive,
            &[t],
            &Map::new(),
            Some("Old."),
        )
        .unwrap();
    }
    submit(s, cs, &agent(), "Old mail.", NOON).unwrap();
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
    approve(s, id(1), &hash(1), NOON).unwrap();
    approve(s, id(2), &hash(2), NOON).unwrap();
    decide(s, id(3), State::Denied, By::Person, Some("No."), NOON).unwrap();
    cs
}

/// Approvals count the decisions of the person on the actions of another
/// author. The state verbs of the person are not proposals. Undo counts
/// the applied actions that an undo reversed, and not the undo itself.
#[tokio::test]
async fn approvals_and_undo_count_the_decisions_of_the_person() {
    let s = store("decide");
    let cs = decided(&s);
    // The account of decided().
    let a = 1;

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

    // A withdraw cancels the two approved actions: the decisions of the
    // person stay in the measure.
    let (code, _) = post(&s, &format!("/api/changesets/cs-{cs}/withdraw"), "").await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(
        get(&s).await["approvals"],
        json!({"decided": 3, "approved": 2})
    );

    // Invariant: an action of an undo (each step, in table ledger_undo) is
    // not an applied change of the measure. A close of a second item gives
    // entry 3. Make entry 2 step 1 of the undo of entry 1, and entry 3 its
    // step 2 (as the archive after an untrash), past the triggers: a state
    // verb has no Gmail undo.
    doc["id"] = json!("c-2");
    s.write(|tx| {
        tx.execute(
            "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
             VALUES ('commitment', ?1, 'open', ?2, 0, 0)",
            params![a, doc.to_string()],
        )?;
        tx.execute(
            "INSERT INTO item_source(item_id, message_id, role) VALUES (2, 1, 'origin')",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    assert_eq!(
        post(&s, "/api/items/c-2/close", "{}").await.0,
        StatusCode::OK
    );
    s.write(|tx| {
        tx.execute_batch(
            "DROP TRIGGER ledger_change; DROP TRIGGER ledger_state;
             DROP TRIGGER ledger_undo_insert; DROP TRIGGER ledger_undone_by;
             UPDATE ledger SET state = 'applied' WHERE id = 2;
             UPDATE ledger SET state = 'undone', undone_by = 2 WHERE id = 1;
             INSERT INTO ledger_undo(action_id, ledger_id, step)
             SELECT action_id, 1, CASE id WHEN 2 THEN 1 ELSE 2 END FROM ledger WHERE id IN (2, 3);",
        )?;
        Ok(())
    })
    .unwrap();
    let m = get(&s).await;
    assert_eq!(m["undo"], json!({"applied": 1, "undone": 1}));
}

/// One action, submitted, then revised (with a comment of the person on
/// revision 1 if `comment`), and the action of revision 2 approved.
fn approve_revision(s: &Store, account: i64, comment: bool) {
    let msg = format!("g-rev-{comment}");
    s.put_messages(
        account,
        &[NewMessage {
            provider_id: msg.clone(),
            thread_provider_id: msg,
            ..Default::default()
        }],
    )
    .unwrap();
    let target: String = s
        .read(|c| Ok(c.query_row("SELECT 'm-' || max(id) FROM message", [], |r| r.get(0))?))
        .unwrap();
    let cs = create(s, &agent(), NOON).unwrap();
    let act = |s: &Store| {
        add_action(
            s,
            cs,
            &agent(),
            Verb::Archive,
            &[target.as_str()],
            &Map::new(),
            Some("Old."),
        )
        .unwrap()
    };
    act(s);
    submit(s, cs, &agent(), "Old mail.", NOON).unwrap();
    if comment {
        s.write(|tx| {
            Ok(tx.execute(
                "INSERT INTO changeset_comment (changeset_id, revision, text, key, at_ms)
                 VALUES (?1, 1, 'Only the newsletters.', 'key-of-the-comment-1', ?2)",
                params![cs, NOON],
            )?)
        })
        .unwrap();
    }
    revise(s, cs, &agent()).unwrap();
    act(s);
    submit(s, cs, &agent(), "Old mail, again.", NOON).unwrap();
    let hash: String = s
        .read(|c| {
            Ok(c.query_row(
                "SELECT hash FROM changeset_action WHERE changeset_id = ?1 AND revision = 2",
                [cs],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    let id = ActionId {
        changeset: cs,
        revision: 2,
        n: 1,
    };
    approve(s, id, &hash, NOON).unwrap();
}

/// Invariant (#494): an approval counts as without an edit only if the
/// person wrote no comment on an earlier revision of the changeset. A
/// revision with no comment of the person still counts.
#[tokio::test]
async fn an_approval_after_a_comment_is_not_without_an_edit() {
    let s = store("revised");
    let a = s.add_account("gmail", "owner@example.com").unwrap();
    approve_revision(&s, a, true);
    approve_revision(&s, a, false);
    assert_eq!(
        get(&s).await["approvals"],
        json!({"decided": 2, "approved": 1})
    );
}

/// Migration 45 corrects an approval after a comment that an older store
/// counted as without an edit.
#[test]
fn migration_45_corrects_an_approval_after_a_comment() {
    let dir = std::env::temp_dir().join(format!(
        "iris-api-measures-old-revised-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    {
        let s = Store::open(&dir).unwrap();
        let a = s.add_account("gmail", "owner@example.com").unwrap();
        approve_revision(&s, a, true);
        s.write(|tx| {
            tx.execute_batch("UPDATE person_decision SET approved = 1; PRAGMA user_version = 44;")?;
            Ok(())
        })
        .unwrap();
    }
    let s = Store::open(&dir).unwrap();
    let m = iris_api::measures::measures(&s, None).unwrap();
    assert_eq!((m.approvals.decided, m.approvals.approved), (1, 0));
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}
