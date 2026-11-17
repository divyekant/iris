//! The write view of the web client (docs/specs/2026-11-17-write.md):
//! POST /api/write, GET /api/write/{cs} and GET /api/threads/{t}/reply.
//! The apply step is tested in `crates/iris-apply/tests/write.rs`; these
//! tests make the draft row by hand.

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use iris_api::Auth;
use iris_store::Store;
use serde_json::{json, Value};
use tower::ServiceExt;

const OWNER: &str = "owner@example.com";

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp(name: &str) -> TempDir {
    let dir = std::env::temp_dir().join(format!("iris-api-write-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    TempDir(dir)
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

/// Account a-1, and thread t-1 with two mails from priya: m-1, then m-2
/// (the newest).
fn store(dir: &TempDir) -> Arc<Store> {
    let s = Store::open(&dir.0).unwrap();
    s.add_account("gmail", OWNER).unwrap();
    s.write(|tx| {
        tx.execute_batch(
            "INSERT INTO thread(account_id, provider_id) VALUES (1, 'th1');
             INSERT INTO message(account_id, thread_id, provider_id, internal_date)
               VALUES (1, 1, 'p1', 1000), (1, 1, 'p2', 2000);
             INSERT INTO header(message_id, from_addr, from_name, to_addrs, subject)
               VALUES (1, 'priya@example.com', 'Priya Shah', 'owner@example.com', 'Q4'),
                      (2, 'priya@example.com', 'Priya Shah', 'owner@example.com', 'Q4');",
        )?;
        Ok(())
    })
    .unwrap();
    Arc::new(s)
}

async fn call(
    store: &Arc<Store>,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let auth = Arc::new(Auth::new("127.0.0.1:4800".parse().unwrap()).unwrap());
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "127.0.0.1:4800")
        .header("authorization", auth.bearer());
    if body.is_some() {
        req = req.header("content-type", "application/json");
    }
    let req = req
        .body(body.map_or_else(Body::empty, |b| Body::from(b.to_string())))
        .unwrap();
    let res = iris_api::router(store.clone(), auth)
        .oneshot(req)
        .await
        .unwrap();
    let code = res.status();
    let body = to_bytes(res.into_body(), 1 << 20).await.unwrap();
    (code, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

fn new_mail(key: &str, flags: Value) -> Value {
    json!({"account": "a-1", "to": ["priya@example.com"], "cc": [], "subject": "Q4",
           "body": "Hi.\n", "key": key, "flags": flags})
}

const KEY: &str = "0123456789abcdef-1";

fn count(s: &Store, sql: &str) -> i64 {
    s.read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?)).unwrap()
}

/// The draft row that the apply makes for the `draft` action of the mark.
fn made(s: &Store) {
    s.write(|tx| {
        let action: i64 = tx.query_row("SELECT action_id FROM write_send", [], |r| r.get(0))?;
        let c = iris_compose::Content {
            to: vec!["priya@example.com".into()],
            cc: vec![],
            subject: "Q4".into(),
            body: "Hi.\n".into(),
        }
        .check()
        .unwrap();
        let raw = iris_compose::compose(OWNER, "iris.1@example.com", &c, None, 1);
        tx.execute(
            "INSERT INTO draft(account_id, provider_id, message_id_hdr, content, hash, raw,
                               action_id, created_at, updated_at)
             VALUES (1, 'r1', 'iris.1@example.com', ?3, ?1, ?2, ?4, 0, 0)",
            rusqlite::params![
                iris_compose::hash(&c, None),
                raw,
                serde_json::to_string(&c).unwrap(),
                action
            ],
        )?;
        Ok(())
    })
    .unwrap();
}

/// A press with the warnings that the engine names writes the mail; the
/// state goes from saving to waiting. A press with other warnings gets 409
/// and the warnings, and writes nothing.
#[tokio::test]
async fn a_press_writes_the_mail_after_its_warnings() {
    let dir = temp("press");
    let s = store(&dir);
    let (code, v) = call(&s, "POST", "/api/write", Some(new_mail(KEY, json!([])))).await;
    assert_eq!(code, StatusCode::CONFLICT, "{v}");
    assert_eq!(v["flags"], json!(["new_recipient"]));
    assert_eq!(v["error"], "Look at the warnings, then press Send again.");
    assert_eq!(count(&s, "SELECT count(*) FROM changeset"), 0);
    let (code, v) = call(
        &s,
        "POST",
        "/api/write",
        Some(new_mail(KEY, v["flags"].clone())),
    )
    .await;
    assert_eq!(code, StatusCode::CREATED, "{v}");
    let cs = v["write"].as_str().unwrap().to_string();
    let (_, st) = call(&s, "GET", &format!("/api/write/{cs}"), None).await;
    assert_eq!(st, json!({"state": "saving"}));
    made(&s);
    let (_, st) = call(&s, "GET", &format!("/api/write/{cs}"), None).await;
    assert_eq!(
        st,
        json!({"state": "saving"}),
        "the draft exists, step 2a did not run"
    );
    iris_changeset::send_written(&s, 1, now()).unwrap();
    let (_, st) = call(&s, "GET", &format!("/api/write/{cs}"), None).await;
    assert_eq!(st["state"], "waiting", "{st}");
    assert_eq!(st["draft"], "d-1");
    assert_eq!(st["sending"], false);
    assert_eq!(st["leaves_at"], Value::Null);
    // The cancel of the outbox stops the write.
    let (code, _) = call(&s, "POST", "/api/outbox/d-1/cancel", None).await;
    assert_eq!(code, StatusCode::OK);
    let (_, st) = call(&s, "GET", &format!("/api/write/{cs}"), None).await;
    assert_eq!(
        st,
        json!({"state": "stopped", "reason": "Cancelled in the outbox.", "draft": "d-1"})
    );
}

/// A second try of one press gets 200 and the same write. The same key
/// with another mail gets 409.
#[tokio::test]
async fn a_second_try_finds_the_first_write() {
    let dir = temp("again");
    let s = store(&dir);
    let body = new_mail(KEY, json!(["new_recipient"]));
    let (code, first) = call(&s, "POST", "/api/write", Some(body.clone())).await;
    assert_eq!(code, StatusCode::CREATED);
    let (code, second) = call(&s, "POST", "/api/write", Some(body.clone())).await;
    assert_eq!((code, &second), (StatusCode::OK, &first));
    let mut other = body;
    other["body"] = json!("Other.\n");
    let (code, v) = call(&s, "POST", "/api/write", Some(other)).await;
    assert_eq!(code, StatusCode::CONFLICT);
    assert_eq!(v["error"], "This key belongs to another mail.");
    assert_eq!(count(&s, "SELECT count(*) FROM write_send"), 1);
}

/// A withdraw before the draft stops the write.
#[tokio::test]
async fn a_withdraw_stops_the_write() {
    let dir = temp("withdraw");
    let s = store(&dir);
    let (_, v) = call(
        &s,
        "POST",
        "/api/write",
        Some(new_mail(KEY, json!(["new_recipient"]))),
    )
    .await;
    let cs = v["write"].as_str().unwrap().to_string();
    let (code, _) = call(&s, "POST", &format!("/api/changesets/{cs}/withdraw"), None).await;
    assert_eq!(code, StatusCode::OK);
    let (_, st) = call(&s, "GET", &format!("/api/write/{cs}"), None).await;
    assert_eq!(
        st,
        json!({"state": "stopped", "reason": "You stopped this mail.", "draft": null})
    );
}

/// Bodies that are not valid get 400 and write nothing.
#[tokio::test]
async fn a_body_that_is_not_valid_writes_nothing() {
    let dir = temp("bad");
    let s = store(&dir);
    let base = new_mail(KEY, json!(["new_recipient"]));
    let mut cases = vec![];
    let mut v = base.clone();
    v["key"] = json!("short");
    cases.push(v);
    let mut v = base.clone();
    v["key"] = json!("0123456789abcdef!!");
    cases.push(v);
    let mut v = base.clone();
    v["thread"] = json!("t-1");
    cases.push(v);
    let mut v = base.clone();
    v.as_object_mut().unwrap().remove("account");
    cases.push(v);
    let mut v = base.clone();
    v.as_object_mut().unwrap().remove("subject");
    cases.push(v);
    let mut v = base.clone();
    v["to"] = json!(["not an address"]);
    cases.push(v);
    let mut v = base.clone();
    v["extra"] = json!(1);
    cases.push(v);
    let mut v = base.clone();
    v["account"] = json!("a-9");
    cases.push(v);
    for body in cases {
        let (code, v) = call(&s, "POST", "/api/write", Some(body.clone())).await;
        assert_eq!(code, StatusCode::BAD_REQUEST, "{body} gave {v}");
    }
    assert_eq!(count(&s, "SELECT count(*) FROM changeset"), 0);
}

/// The reply form names the newest mail and its sender. A press with an
/// older parent gets 409 and writes nothing.
#[tokio::test]
async fn a_reply_names_the_newest_mail() {
    let dir = temp("reply");
    let s = store(&dir);
    let (code, v) = call(&s, "GET", "/api/threads/t-1/reply", None).await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(v["account"], "a-1");
    assert_eq!(v["parent"]["id"], "m-2");
    assert_eq!(v["to"], json!(["priya@example.com"]));
    assert_eq!(v["subject"], "Re: Q4");
    let reply = |parent: &str| {
        json!({"thread": "t-1", "parent": parent, "to": ["priya@example.com"], "cc": [],
               "body": "Yes.\n", "key": KEY, "flags": ["new_recipient", "first_contact"]})
    };
    let (code, v) = call(&s, "POST", "/api/write", Some(reply("m-1"))).await;
    assert_eq!(code, StatusCode::CONFLICT, "{v}");
    assert_eq!(
        v["error"],
        "A new mail came into this thread. Read it, then send again."
    );
    assert_eq!(count(&s, "SELECT count(*) FROM changeset"), 0);
    let (code, v) = call(&s, "POST", "/api/write", Some(reply("m-2"))).await;
    // The flags of a reply are the engine's: the client shows the 409 list.
    let v = if code == StatusCode::CONFLICT {
        let mut b = reply("m-2");
        b["flags"] = v["flags"].clone();
        call(&s, "POST", "/api/write", Some(b)).await.1
    } else {
        v
    };
    assert!(v["write"].is_string(), "{v}");
    // A thread with no mail, and a thread that does not exist.
    let (code, _) = call(&s, "GET", "/api/threads/t-9/reply", None).await;
    assert_eq!(code, StatusCode::NOT_FOUND);
    s.write(|tx| {
        Ok(tx.execute(
            "INSERT INTO thread(account_id, provider_id) VALUES (1, 'th2')",
            [],
        )?)
    })
    .unwrap();
    let (code, _) = call(&s, "GET", "/api/threads/t-2/reply", None).await;
    assert_eq!(code, StatusCode::CONFLICT);
}

/// The person wrote the newest mail: the reply goes to its recipients.
#[tokio::test]
async fn a_reply_after_your_own_mail_goes_to_its_recipients() {
    let dir = temp("own");
    let s = store(&dir);
    s.write(|tx| {
        tx.execute_batch(
            "INSERT INTO message(account_id, thread_id, provider_id, internal_date)
               VALUES (1, 1, 'p3', 3000);
             INSERT INTO header(message_id, from_addr, to_addrs, subject)
               VALUES (3, 'owner@example.com', 'priya@example.com,bad address,sam@example.com', 'Re: Q4');",
        )?;
        Ok(())
    })
    .unwrap();
    let (_, v) = call(&s, "GET", "/api/threads/t-1/reply", None).await;
    assert_eq!(v["to"], json!(["priya@example.com", "sam@example.com"]));
    assert_eq!(v["subject"], "Re: Q4");
}

/// A changeset that is not a write has no write state.
#[tokio::test]
async fn only_a_write_has_a_state() {
    let dir = temp("state");
    let s = store(&dir);
    for id in ["cs-1", "cs-x", "t-1"] {
        let (code, _) = call(&s, "GET", &format!("/api/write/{id}"), None).await;
        assert_eq!(code, StatusCode::NOT_FOUND, "{id}");
    }
}

/// A mark 24 hours old with no send is stopped: step 2a makes no send for
/// it (spec section 4). The test moves the press back past the trigger
/// that keeps the mark fixed.
#[tokio::test]
async fn an_old_write_is_stopped() {
    let dir = temp("old");
    let s = store(&dir);
    let (_, v) = call(
        &s,
        "POST",
        "/api/write",
        Some(new_mail(KEY, json!(["new_recipient"]))),
    )
    .await;
    let cs = v["write"].as_str().unwrap().to_string();
    made(&s);
    s.write(|tx| {
        tx.execute_batch(
            "DROP TRIGGER write_send_update;
             UPDATE write_send SET created_at = created_at - 86400000;",
        )?;
        Ok(())
    })
    .unwrap();
    let (_, st) = call(&s, "GET", &format!("/api/write/{cs}"), None).await;
    assert_eq!(
        st,
        json!({"state": "stopped", "reason": "Iris did not send this mail in 24 hours, so it stopped. Look for the draft in your Gmail drafts.", "draft": "d-1"})
    );
}
