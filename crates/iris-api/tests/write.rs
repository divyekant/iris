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
            ..Default::default()
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
                json!({"content": c, "reply": null}).to_string(),
                action
            ],
        )?;
        // The ledger entry of the create: the send of the mark binds to its
        // hash (draft change spec, section 2).
        let ops = serde_json::json!({"call": "drafts.create", "message_id": "iris.1@example.com",
                                     "date": 1, "hash": iris_compose::hash(&c, None), "thread": null});
        tx.execute(
            "INSERT INTO ledger(action_id, account_id, ops, state, undo, at)
             VALUES (?1, 1, ?2, 'writing', 'none', 0)",
            rusqlite::params![action, ops.to_string()],
        )?;
        tx.execute("UPDATE ledger SET call = 1 WHERE action_id = ?1", [action])?;
        tx.execute("UPDATE ledger SET state = 'applied' WHERE action_id = ?1", [action])?;
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
    iris_changeset::send_written(&s, 1, &now).unwrap();
    let (_, st) = call(&s, "GET", &format!("/api/write/{cs}"), None).await;
    assert_eq!(st["state"], "waiting", "{st}");
    assert_eq!(st["draft"], "d-1");
    assert_eq!(st["sending"], false);
    assert_eq!(st["leaves_at"], Value::Null);
    assert_eq!(st["bcc_check"], Value::Null);
    // A send with Bcc says that it waits for the Bcc gate of its provider.
    s.write(|tx| {
        let c = json!({"content": {"to": ["priya@example.com"], "cc": [], "bcc": ["sam@example.org"],
                                   "subject": "Q4", "body": "Hi.\n"}, "reply": null});
        tx.execute("UPDATE draft SET content = ?1 WHERE id = 1", [c.to_string()])?;
        tx.execute("UPDATE send_switch SET on_at = 0 WHERE id = 1", [])?;
        Ok(())
    })
    .unwrap();
    let (_, st) = call(&s, "GET", &format!("/api/write/{cs}"), None).await;
    assert_eq!(
        (&st["state"], &st["bcc_check"]),
        (&json!("waiting"), &json!("gmail")),
        "{st}"
    );
    // The cancel of the outbox stops the write.
    let (code, _) = call(&s, "POST", "/api/outbox/d-1/cancel", None).await;
    assert_eq!(code, StatusCode::OK);
    let (_, st) = call(&s, "GET", &format!("/api/write/{cs}"), None).await;
    assert_eq!(
        st,
        json!({"state": "stopped", "reason": "Cancelled in the outbox.", "draft": "d-1",
               "change": "update", "newer": false})
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

/// Step 2a runs after the last read of the client (`saving`) and before
/// the withdraw: the withdraw stops the send too (spec section 4).
#[tokio::test]
async fn a_withdraw_after_step_2a_stops_the_send() {
    let dir = temp("withdraw-2a");
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
    assert_eq!(iris_changeset::send_written(&s, 1, &now).unwrap(), 1);
    let (code, _) = call(&s, "POST", &format!("/api/changesets/{cs}/withdraw"), None).await;
    assert_eq!(code, StatusCode::OK);
    let (_, st) = call(&s, "GET", &format!("/api/write/{cs}"), None).await;
    assert_eq!(st["state"], "stopped");
    assert_eq!(
        st["reason"],
        "You stopped this mail. The draft stays in your Gmail drafts."
    );
    assert_eq!(
        count(
            &s,
            "SELECT count(*) FROM changeset_action WHERE verb = 'send' AND state = 'cancelled'"
        ),
        1
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
    // No mail with no text (issue 741).
    for body in ["", " \n\t\r\n"] {
        let mut v = base.clone();
        v["body"] = json!(body);
        cases.push(v);
    }
    let mut v = base.clone();
    v["account"] = json!("a-9");
    cases.push(v);
    // Target IDs that are not IDs of their kind (issue #499).
    for bad in [
        "bad",
        "",
        "a-",
        "a-x",
        "a-0",
        "a--1",
        "t-1",
        "m-1",
        "cs-1",
        "a-1 ",
        " a-1",
        "a-99999999999999999999",
    ] {
        let mut v = base.clone();
        v["account"] = json!(bad);
        cases.push(v);
    }
    for bad in [
        "bad",
        "",
        "t-",
        "t-x",
        "t-0",
        "a-1",
        "m-1",
        "t-1x",
        "t-99999999999999999999",
    ] {
        let mut v = base.clone();
        v.as_object_mut().unwrap().remove("account");
        v.as_object_mut().unwrap().remove("subject");
        v["thread"] = json!(bad);
        v["parent"] = json!("m-2");
        cases.push(v);
    }
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

/// The start values hold the conversation above the reply form: the newest
/// answerable mails, oldest first, with the parent last, and the count of
/// the older ones. A draft or a mail in the trash is not part of it. The
/// text of a long mail is cut.
#[tokio::test]
async fn a_reply_shows_the_conversation() {
    // `write::CONVERSATION` and `write::MAIL_TEXT` of the API.
    const CONVERSATION: usize = 20;
    const MAIL_TEXT: usize = 4000;
    let dir = temp("talk");
    let s = store(&dir);
    let n = CONVERSATION as i64 + 3;
    let long = "x".repeat(MAIL_TEXT + 5);
    s.write(|tx| {
        tx.execute_batch(
            "INSERT INTO label(account_id, provider_id, name, kind) VALUES
               (1, 'SENT', 'SENT', 'system'), (1, 'DRAFT', 'DRAFT', 'system'),
               (1, 'TRASH', 'TRASH', 'system'), (1, 'UNSENT', 'UNSENT', 'system'),
               (1, 'SENT_COPY', 'SENT_COPY', 'system');",
        )?;
        // Messages 3 to n: mail of the person (label SENT) with a long text.
        for i in 3..=n {
            tx.execute(
                "INSERT INTO message(account_id, thread_id, provider_id, internal_date)
                   VALUES (1, 1, ?1, ?2)",
                rusqlite::params![format!("p{i}"), i * 1000],
            )?;
            tx.execute(
                "INSERT INTO body(message_id, subject, sender, text) VALUES (?1, 'Re: Q4', 'owner', ?2)",
                rusqlite::params![i, long],
            )?;
            tx.execute(
                "INSERT INTO header(message_id, from_addr, to_addrs, subject)
                   VALUES (?1, 'owner@example.com', 'priya@example.com', 'Re: Q4')",
                [i],
            )?;
            tx.execute(
                "INSERT INTO message_label(message_id, label_id)
                   SELECT ?1, id FROM label WHERE provider_id = 'SENT'",
                [i],
            )?;
        }
        tx.execute("INSERT INTO link_check(message_id) VALUES (?1)", [n])?;
        // A newer draft, a newer mail in the trash, an IMAP draft and a
        // copy in a Sent folder.
        for (i, label) in [
            (n + 1, "DRAFT"),
            (n + 2, "TRASH"),
            (n + 3, "UNSENT"),
            (n + 4, "SENT_COPY"),
        ] {
            tx.execute(
                "INSERT INTO message(account_id, thread_id, provider_id, internal_date)
                   VALUES (1, 1, ?1, ?2)",
                rusqlite::params![format!("p{i}"), i * 1000],
            )?;
            tx.execute(
                "INSERT INTO message_label(message_id, label_id)
                   SELECT ?1, id FROM label WHERE provider_id = ?2",
                rusqlite::params![i, label],
            )?;
        }
        Ok(())
    })
    .unwrap();
    let (code, v) = call(&s, "GET", "/api/threads/t-1/reply", None).await;
    assert_eq!(code, StatusCode::OK, "{v}");
    let mails = v["conversation"].as_array().unwrap();
    assert_eq!(mails.len(), CONVERSATION);
    assert_eq!(v["earlier"], n - CONVERSATION as i64);
    let ids: Vec<&str> = mails.iter().map(|m| m["id"].as_str().unwrap()).collect();
    let want: Vec<String> = (n + 1 - CONVERSATION as i64..=n)
        .map(|i| format!("m-{i}"))
        .collect();
    assert_eq!(ids, want);
    assert_eq!(v["parent"]["id"], *ids.last().unwrap());
    let last = mails.last().unwrap();
    assert_eq!(last["you"], true);
    // The parent waits for its link check: it counts as hostile mail.
    assert_eq!(last["hostile"], true);
    assert_eq!(mails[mails.len() - 2]["hostile"], false);
    assert_eq!(last["cut"], true);
    assert_eq!(
        last["text"].as_str().unwrap().chars().count(),
        MAIL_TEXT + 1
    );
    assert_eq!(v["topic"], "Re: Q4");
    // A short thread: the two mails of the fixture, from priya.
    let dir = temp("talk-short");
    let s = store(&dir);
    s.write(|tx| {
        Ok(tx.execute(
            r#"UPDATE message SET hostile = '["link_mismatch"]' WHERE id = 1"#,
            [],
        )?)
    })
    .unwrap();
    let (_, v) = call(&s, "GET", "/api/threads/t-1/reply", None).await;
    assert_eq!(v["earlier"], 0);
    assert_eq!(v["conversation"][0]["hostile"], true);
    assert_eq!(v["conversation"][1]["hostile"], false);
    assert_eq!(v["conversation"][0]["id"], "m-1");
    assert_eq!(v["conversation"][1]["id"], "m-2");
    assert_eq!(v["conversation"][1]["you"], false);
    assert_eq!(v["conversation"][1]["from"]["name"], "Priya Shah");
}

/// Add the newest mail of thread 1 (message 3), from `from` to `to`, with
/// the label `SENT` if `sent`.
fn newest(s: &Store, from: &str, to: &str, sent: bool) {
    s.write(|tx| {
        tx.execute(
            "INSERT INTO message(account_id, thread_id, provider_id, internal_date)
               VALUES (1, 1, 'p3', 3000)",
            [],
        )?;
        tx.execute(
            "INSERT INTO header(message_id, from_addr, to_addrs, subject)
               VALUES (3, ?1, ?2, 'Re: Q4')",
            [from, to],
        )?;
        if sent {
            tx.execute_batch(
                "INSERT INTO label(account_id, provider_id, name, kind)
                   VALUES (1, 'SENT', 'SENT', 'system');
                 INSERT INTO message_label(message_id, label_id)
                   SELECT 3, id FROM label WHERE provider_id = 'SENT';",
            )?;
        }
        Ok(())
    })
    .unwrap();
}

/// The person wrote the newest mail (the label `SENT` says so): the reply
/// goes to its recipients. Each entry of the list counts, with a display
/// name, a quoted comma or a comment too; an entry with no address does
/// not.
#[tokio::test]
async fn a_reply_after_your_own_mail_goes_to_its_recipients() {
    let lists = [
        (
            "priya@example.com,bad address,sam@example.com",
            vec!["priya@example.com", "sam@example.com"],
        ),
        (
            "Priya Shah <priya@example.com>, \"Okafor, Sam\" <SAM@example.com>",
            vec!["priya@example.com", "sam@example.com"],
        ),
        (
            "priya@example.com (Priya), <sam@example.com>",
            vec!["priya@example.com", "sam@example.com"],
        ),
        ("", vec![]),
    ];
    for (i, (to, want)) in lists.into_iter().enumerate() {
        let dir = temp(&format!("own-{i}"));
        let s = store(&dir);
        newest(&s, OWNER, to, true);
        let (_, v) = call(&s, "GET", "/api/threads/t-1/reply", None).await;
        assert_eq!(v["to"], json!(want), "{to}");
        assert_eq!(v["subject"], "Re: Q4");
    }
}

/// For random text in the `To` list of mail of the person, each start
/// address of the reply passes the address check of a draft (R3-01): text
/// that is not a list gives no address that is not valid.
#[tokio::test]
async fn property_each_start_address_is_valid() {
    const SIGNS: &[&str] = &[
        "a",
        "Z",
        "@",
        ".",
        ",",
        ";",
        ":",
        "<",
        ">",
        "(",
        ")",
        "\"",
        " ",
        "é",
        "example.com",
        "b@example.org",
    ];
    let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut next = |n: usize| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        (x % n as u64) as usize
    };
    let dir = temp("own-any");
    let s = store(&dir);
    newest(&s, OWNER, "", true);
    for _ in 0..300 {
        let n = next(16);
        let text: String = (0..n).map(|_| SIGNS[next(SIGNS.len())]).collect();
        s.write(|tx| {
            Ok(tx.execute(
                "UPDATE header SET to_addrs = ?1 WHERE message_id = 3",
                [&text],
            )?)
        })
        .unwrap();
        let (_, v) = call(&s, "GET", "/api/threads/t-1/reply", None).await;
        for a in v["to"].as_array().unwrap() {
            let a = a.as_str().unwrap().to_string();
            let c = iris_compose::Content {
                to: vec![a.clone()],
                cc: vec![],
                subject: "s".into(),
                body: "b".into(),
                ..Default::default()
            };
            assert!(c.check().is_ok(), "{text:?} gave {a:?}");
        }
    }
}

/// From can be forged: mail that comes in with the address of the person
/// in From (no label `SENT`) is mail of a stranger. The reply goes to that
/// From address, never to the recipients that the stranger chose.
#[tokio::test]
async fn a_forged_from_does_not_choose_the_recipients() {
    let dir = temp("forged");
    let s = store(&dir);
    newest(&s, OWNER, "victim@example.com, other@example.com", false);
    let (_, v) = call(&s, "GET", "/api/threads/t-1/reply", None).await;
    assert_eq!(v["to"], json!([OWNER]));
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
    let mut old = json!({"state": "stopped", "reason": "Iris did not send this mail in 24 hours, so it stopped. Look for the draft in your Gmail drafts.", "draft": "d-1", "change": "update", "newer": false});
    let (_, st) = call(&s, "GET", &format!("/api/write/{cs}"), None).await;
    assert_eq!(st, old);
    // A second stop cause does not hide the age: the draft also changed in
    // Gmail, so Iris cannot change it.
    s.write(|tx| Ok(tx.execute("UPDATE draft SET foreign_at = 1", [])?))
        .unwrap();
    let (_, st) = call(&s, "GET", &format!("/api/write/{cs}"), None).await;
    old["change"] = Value::Null;
    assert_eq!(st, old);
}

/// One raw request body, with no JSON.
async fn upload(store: &Arc<Store>, bytes: Vec<u8>) -> (StatusCode, Value) {
    let auth = Arc::new(Auth::new("127.0.0.1:4800".parse().unwrap()).unwrap());
    let req = Request::builder()
        .method("POST")
        .uri("/api/attachments")
        .header("host", "127.0.0.1:4800")
        .header("authorization", auth.bearer())
        .header("content-type", "application/octet-stream")
        .body(Body::from(bytes))
        .unwrap();
    let res = iris_api::router(store.clone(), auth)
        .oneshot(req)
        .await
        .unwrap();
    let code = res.status();
    let body = to_bytes(res.into_body(), 1 << 20).await.unwrap();
    (code, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

/// Send content spec, sections 2 and 6.1: an upload writes the file to
/// the attachment folder; a file over the limit gets 413; the press of
/// Send takes the entry, and the same key with another file gets 409.
#[tokio::test]
async fn a_file_goes_into_the_folder_and_into_the_press() {
    let dir = temp("file");
    let s = store(&dir);
    let (code, v) = upload(&s, b"%PDF-1 numbers".to_vec()).await;
    assert_eq!(code, StatusCode::CREATED, "{v}");
    let sha = v["sha256"].as_str().unwrap().to_string();
    assert_eq!(v["size"], 14);
    assert_eq!(
        std::fs::read(s.attachment_path(&sha).unwrap()).unwrap(),
        b"%PDF-1 numbers"
    );
    let (code, _) = upload(&s, Vec::new()).await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    let (code, _) = upload(&s, vec![b'x'; iris_api::ATTACHMENT_LIMIT + 1]).await;
    assert_eq!(code, StatusCode::PAYLOAD_TOO_LARGE);

    let mut body = new_mail(KEY, json!(["new_recipient", "attachment"]));
    body["body"] = json!("The numbers are attached.\n");
    body["attachments"] =
        json!([{"name": "q4.pdf", "type": "application/pdf", "sha256": sha, "size": 14}]);
    let (code, v) = call(&s, "POST", "/api/write", Some(body.clone())).await;
    // A draft with a file has no "no file attached" warning.
    assert_eq!(code, StatusCode::CONFLICT, "{v}");
    assert_eq!(v["flags"], json!(["new_recipient"]));
    body["flags"] = v["flags"].clone();
    let (code, v) = call(&s, "POST", "/api/write", Some(body.clone())).await;
    assert_eq!(code, StatusCode::CREATED, "{v}");
    let args: String = s
        .read(|c| {
            Ok(c.query_row(
                "SELECT args FROM changeset_action WHERE verb = 'draft'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert!(args.contains("q4.pdf"), "{args}");
    body["attachments"][0]["name"] = json!("other.pdf");
    let (code, v) = call(&s, "POST", "/api/write", Some(body)).await;
    assert_eq!(code, StatusCode::CONFLICT);
    assert_eq!(v["error"], "This key belongs to another mail.");
}

/// The write of a change of d-1 (draft change spec, section 2), with
/// `body` as the new text: the first write is withdrawn after its draft,
/// the change is approved and applied, and the person pressed Send. The
/// draft row gets the new content, as the apply writes it. Gives the
/// changeset ID of the change.
fn changed_write(s: &Store, body: &str) -> String {
    use iris_changeset::{Author, AuthorKind, Verb};
    let who = Author {
        kind: AuthorKind::Person,
        name: None,
        session: None,
        job: None,
    };
    let mut args = new_mail(KEY, json!([]));
    let args = args.as_object_mut().unwrap();
    for k in ["account", "key", "flags"] {
        args.remove(k);
    }
    args.insert("body".into(), json!(body));
    let cs = iris_changeset::create(s, &who, now()).unwrap();
    iris_changeset::add_action(s, cs, &who, Verb::Draft, &["d-1"], args, None).unwrap();
    iris_changeset::submit(s, cs, &who, "Change.", now()).unwrap();
    s.write(|tx| {
        let (action, state, reason): (i64, String, Option<String>) = tx.query_row(
            "SELECT id, state, reason FROM changeset_action WHERE changeset_id = ?1",
            [cs],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
        assert!(
            ["proposed", "in_review"].contains(&state.as_str()),
            "{state} {reason:?}"
        );
        tx.execute(
            "UPDATE changeset_action SET state = 'approved', decided_by = 'person',
               decided_at = ?2 WHERE id = ?1",
            rusqlite::params![action, now()],
        )?;
        // The press marks the approved action, before its apply.
        tx.execute(
            "INSERT INTO write_send(action_id, request_key, request_hash, flags, created_at)
             VALUES (?1, ?2, ?3, '[\"new_recipient\"]', ?4)",
            rusqlite::params![action, "fedcba9876543210-2", "a".repeat(64), now()],
        )?;
        let old: String = tx.query_row("SELECT hash FROM draft WHERE id = 1", [], |r| r.get(0))?;
        let c = iris_compose::Content {
            to: vec!["priya@example.com".into()],
            cc: vec![],
            subject: "Q4".into(),
            body: body.into(),
            ..Default::default()
        }
        .check()
        .unwrap();
        let hash = iris_compose::hash(&c, None);
        let raw = iris_compose::compose(OWNER, "iris.1@example.com", &c, None, 2);
        let ops = json!({"call": "drafts.update", "draft": 1, "provider_id": "r1",
                         "message_id": "iris.1@example.com", "date": 2, "pre_hash": old,
                         "hash": hash, "thread": null});
        tx.execute(
            "INSERT INTO ledger(action_id, account_id, ops, state, undo, at)
             VALUES (?1, 1, ?2, 'writing', 'inverse', 0)",
            rusqlite::params![action, ops.to_string()],
        )?;
        tx.execute("UPDATE ledger SET call = 1 WHERE action_id = ?1", [action])?;
        tx.execute(
            "UPDATE ledger SET state = 'applied' WHERE action_id = ?1",
            [action],
        )?;
        tx.execute(
            "UPDATE draft SET content = ?1, hash = ?2, raw = ?3 WHERE id = 1",
            rusqlite::params![json!({"content": c, "reply": null}).to_string(), hash, raw],
        )?;
        Ok(())
    })
    .unwrap();
    format!("cs-{cs}")
}

/// A first write that made d-1 and then was withdrawn, so that d-1 has no
/// send that waits.
async fn first_write(s: &Arc<Store>) {
    let (_, v) = call(
        s,
        "POST",
        "/api/write",
        Some(new_mail(KEY, json!(["new_recipient"]))),
    )
    .await;
    let cs = v["write"].as_str().unwrap().to_string();
    made(s);
    let (code, _) = call(s, "POST", &format!("/api/changesets/{cs}/withdraw"), None).await;
    assert_eq!(code, StatusCode::OK);
}

/// The state of the write of a change names the draft that the change
/// names, once the change applied, and reaches `waiting` after step 2a.
#[tokio::test]
async fn a_write_of_a_change_waits_with_its_draft() {
    let dir = temp("change-wait");
    let s = store(&dir);
    first_write(&s).await;
    let cs = changed_write(&s, "Changed.\n");
    let (_, st) = call(&s, "GET", &format!("/api/write/{cs}"), None).await;
    assert_eq!(st, json!({"state": "saving"}));
    assert_eq!(iris_changeset::send_written(&s, 1, &now).unwrap(), 1);
    let (_, st) = call(&s, "GET", &format!("/api/write/{cs}"), None).await;
    assert_eq!(st["state"], "waiting", "{st}");
    assert_eq!(st["draft"], "d-1");
}

/// The send of a change binds to the content of the press: a draft row
/// with other content stops the write with the reason, and the draft.
#[tokio::test]
async fn a_write_of_a_change_stops_on_other_content() {
    let dir = temp("change-other");
    let s = store(&dir);
    first_write(&s).await;
    let cs = changed_write(&s, "Changed.\n");
    // The row then holds other valid content, as a refresh from Gmail
    // writes it.
    let c = iris_compose::Content {
        to: vec!["priya@example.com".into()],
        cc: vec![],
        subject: "Q4".into(),
        body: "Other.\n".into(),
        ..Default::default()
    }
    .check()
    .unwrap();
    s.write(|tx| {
        Ok(tx.execute(
            "UPDATE draft SET content = ?1, hash = ?2, raw = ?3 WHERE id = 1",
            rusqlite::params![
                json!({"content": c, "reply": null}).to_string(),
                iris_compose::hash(&c, None),
                iris_compose::compose(OWNER, "iris.1@example.com", &c, None, 3)
            ],
        )?)
    })
    .unwrap();
    assert_eq!(iris_changeset::send_written(&s, 1, &now).unwrap(), 1);
    let (_, st) = call(&s, "GET", &format!("/api/write/{cs}"), None).await;
    assert_eq!(
        st,
        json!({"state": "stopped", "reason": iris_changeset::DRAFT_CHANGED, "draft": "d-1",
               "change": "update", "newer": false})
    );
}

/// A write of a new mail that made d-1, then was withdrawn: no send of
/// d-1 waits. Gives its changeset ID.
async fn stopped_write(s: &Arc<Store>) -> String {
    let (_, v) = call(
        s,
        "POST",
        "/api/write",
        Some(new_mail(KEY, json!(["new_recipient"]))),
    )
    .await;
    let cs = v["write"].as_str().unwrap().to_string();
    made(s);
    let (code, _) = call(s, "POST", &format!("/api/changesets/{cs}/withdraw"), None).await;
    assert_eq!(code, StatusCode::OK);
    cs
}

/// A stopped write with a Gmail draft offers a change in place, and the
/// form reads the content of the draft (draft change spec, section 5).
/// A press with the draft targets the draft, not the account.
#[tokio::test]
async fn a_stopped_write_changes_its_draft() {
    let dir = temp("change-press");
    let s = store(&dir);
    let cs = stopped_write(&s).await;
    let (_, st) = call(&s, "GET", &format!("/api/write/{cs}"), None).await;
    assert_eq!(st["change"], "update", "{st}");
    let (code, d) = call(&s, "GET", "/api/drafts/d-1", None).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(
        d,
        json!({"draft": "d-1", "account": "a-1", "thread": null, "to": ["priya@example.com"],
               "cc": [], "bcc": [], "subject": "Q4", "body": "Hi.\n", "attachments": [],
               "change": "update", "newer": false})
    );
    let mut body = new_mail("0123456789abcdef-2", json!(["new_recipient"]));
    body["body"] = json!("Changed.\n");
    body["draft"] = json!("d-1");
    let (code, v) = call(&s, "POST", "/api/write", Some(body)).await;
    assert_eq!(code, StatusCode::CREATED, "{v}");
    let targets: String = s
        .read(|c| {
            Ok(c.query_row(
                "SELECT a.targets FROM write_send w JOIN changeset_action a ON a.id = w.action_id
                 ORDER BY w.action_id DESC LIMIT 1",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(targets, r#"{"drafts":["d-1"]}"#);
    // The draft waits for the change now: the first write offers none.
    let (_, st) = call(&s, "GET", &format!("/api/write/{cs}"), None).await;
    assert_eq!(st["change"], Value::Null, "{st}");
    let (_, d) = call(&s, "GET", "/api/drafts/d-1", None).await;
    assert_eq!(
        d["change"], st["change"],
        "the draft and the write agree: {d}"
    );
    // A draft that the person changed into a form that Iris does not write
    // has no content to show.
    s.write(|tx| Ok(tx.execute("UPDATE draft SET foreign_at = 1 WHERE id = 1", [])?))
        .unwrap();
    let (code, _) = call(&s, "GET", "/api/drafts/d-1", None).await;
    assert_eq!(code, StatusCode::NOT_FOUND);
}

/// A draft field that is not a draft of the account or the thread of the
/// press: 400, nothing written. The key and the draft are in the request
/// hash: the same key with another draft is a 409.
#[tokio::test]
async fn a_press_names_a_draft_of_its_own_mail() {
    let dir = temp("change-other");
    let s = store(&dir);
    stopped_write(&s).await;
    s.add_account("gmail", "other@example.com").unwrap();
    let before = count(&s, "SELECT count(*) FROM changeset");
    for (account, draft) in [("a-2", "d-1"), ("a-1", "d-9"), ("a-1", "t-1")] {
        let mut body = new_mail("0123456789abcdef-3", json!(["new_recipient"]));
        body["account"] = json!(account);
        body["draft"] = json!(draft);
        let (code, v) = call(&s, "POST", "/api/write", Some(body)).await;
        assert_eq!(code, StatusCode::BAD_REQUEST, "{account} {draft} {v}");
    }
    let reply = json!({"thread": "t-1", "parent": "m-2", "to": ["priya@example.com"],
                           "cc": [], "body": "Hi.\n", "key": "0123456789abcdef-4",
                           "flags": [], "draft": "d-1"});
    let (code, v) = call(&s, "POST", "/api/write", Some(reply.clone())).await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "{v}");
    assert_eq!(count(&s, "SELECT count(*) FROM changeset"), before);
    let mut body = new_mail("0123456789abcdef-5", json!(["new_recipient"]));
    body["draft"] = json!("d-1");
    let (code, _) = call(&s, "POST", "/api/write", Some(body.clone())).await;
    assert_eq!(code, StatusCode::CREATED);
    body.as_object_mut().unwrap().remove("draft");
    let (code, v) = call(&s, "POST", "/api/write", Some(body)).await;
    assert_eq!(
        (code, &v["error"]),
        (StatusCode::CONFLICT, &json!(KEY_TAKEN))
    );
}

const KEY_TAKEN: &str = "This key belongs to another mail.";

/// The `change` of a stopped write (draft change spec, section 5): none
/// for a sent draft, `new` for a draft of an account that is not Gmail,
/// none for a draft of any provider that Iris does not write, and none
/// for a Gmail draft whose attachment file is gone.
#[tokio::test]
async fn a_stopped_write_says_how_its_draft_can_change() {
    let dir = temp("change-kinds");
    let s = store(&dir);
    let cs = stopped_write(&s).await;
    let set = |sql: &'static str| s.write(|tx| Ok(tx.execute_batch(sql)?)).unwrap();
    set("UPDATE draft SET sent_at = 1, sent_message = 's1' WHERE id = 1");
    assert_eq!(change(&s, &cs).await, Value::Null);
    set(
        "UPDATE draft SET sent_at = NULL, sent_message = NULL WHERE id = 1;
         UPDATE account SET provider = 'outlook' WHERE id = 1;",
    );
    assert_eq!(change(&s, &cs).await, "new");
    set("UPDATE draft SET foreign_at = 1 WHERE id = 1");
    assert_eq!(change(&s, &cs).await, Value::Null);
    // An attachment of the draft whose file is not in the attachment
    // folder: a new draft does not need the old file, a change in place
    // does (draft change spec, section 1).
    set("UPDATE draft SET foreign_at = NULL WHERE id = 1");
    assert_eq!(change(&s, &cs).await, "new");
    set(
        "UPDATE draft SET content = json_set(content, '$.content.attachments',
           json('[{\"name\": \"q4.pdf\", \"type\": \"application/pdf\",
                   \"sha256\": \"0000000000000000000000000000000000000000000000000000000000000000\",
                   \"size\": 14}]')) WHERE id = 1",
    );
    assert_eq!(change(&s, &cs).await, "new");
    set("UPDATE account SET provider = 'gmail' WHERE id = 1");
    assert_eq!(change(&s, &cs).await, Value::Null);
}

async fn change(s: &Arc<Store>, cs: &str) -> Value {
    call(s, "GET", &format!("/api/write/{cs}"), None).await.1["change"].clone()
}

/// A stopped reply: its draft answers the parent of the press, m-2, also
/// when the mirror has no Message-ID of m-2. The change ends when the
/// thread gets a newer mail, for each provider. A press of the draft with
/// another parent of the same thread: 400, nothing written.
#[tokio::test]
async fn a_stopped_reply_changes_only_for_its_parent() {
    let dir = temp("change-reply");
    let s = store(&dir);
    let reply = json!({"thread": "t-1", "parent": "m-2", "to": ["priya@example.com"],
                       "cc": [], "body": "Hi.\n", "key": KEY, "flags": ["new_recipient"]});
    let (code, v) = call(&s, "POST", "/api/write", Some(reply)).await;
    assert_eq!(code, StatusCode::CREATED, "{v}");
    let cs = v["write"].as_str().unwrap().to_string();
    made(&s);
    let set = |sql: &'static str| s.write(|tx| Ok(tx.execute_batch(sql)?)).unwrap();
    set("UPDATE draft SET thread_id = 1 WHERE id = 1;
         UPDATE header SET message_id_hdr = NULL WHERE message_id = 2;");
    let (code, _) = call(&s, "POST", &format!("/api/changesets/{cs}/withdraw"), None).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(change(&s, &cs).await, "update");
    let (_, st) = call(&s, "GET", &format!("/api/write/{cs}"), None).await;
    assert_eq!(st["newer"], false, "{st}");
    set("UPDATE account SET provider = 'outlook' WHERE id = 1");
    assert_eq!(change(&s, &cs).await, "new");
    set("UPDATE account SET provider = 'gmail' WHERE id = 1;
         INSERT INTO message(account_id, thread_id, provider_id, internal_date)
           VALUES (1, 1, 'p3', 3000);
         INSERT INTO header(message_id, from_addr, to_addrs, subject, message_id_hdr)
           VALUES (3, 'priya@example.com', 'owner@example.com', 'Q4', '<m3@example.com>');");
    assert_eq!(change(&s, &cs).await, Value::Null);
    // The view names the newer mail (draft change spec, section 5): the
    // write state and the draft read agree.
    let (_, st) = call(&s, "GET", &format!("/api/write/{cs}"), None).await;
    let (_, d) = call(&s, "GET", "/api/drafts/d-1", None).await;
    assert_eq!(
        (&st["newer"], &d["newer"]),
        (&json!(true), &json!(true)),
        "{st} {d}"
    );
    // With no mail to answer in the thread, no mail is newer either.
    set("UPDATE message SET gone_at = 1 WHERE thread_id = 1");
    let (_, st) = call(&s, "GET", &format!("/api/write/{cs}"), None).await;
    let (_, d) = call(&s, "GET", "/api/drafts/d-1", None).await;
    assert_eq!(
        (&st["newer"], &d["newer"], &d["change"]),
        (&json!(false), &json!(false), &Value::Null),
        "{st} {d}"
    );
    set("UPDATE message SET gone_at = NULL WHERE thread_id = 1");
    set("UPDATE account SET provider = 'outlook' WHERE id = 1");
    assert_eq!(change(&s, &cs).await, Value::Null);
    set("UPDATE account SET provider = 'gmail' WHERE id = 1");
    let before = count(&s, "SELECT count(*) FROM changeset");
    let press = json!({"thread": "t-1", "parent": "m-3", "to": ["priya@example.com"],
                       "cc": [], "body": "Hi.\n", "key": "0123456789abcdef-6",
                       "flags": ["new_recipient"], "draft": "d-1"});
    let (code, v) = call(&s, "POST", "/api/write", Some(press)).await;
    assert_eq!(
        (code, &v["error"]),
        (
            StatusCode::BAD_REQUEST,
            &json!("The draft does not answer this message.")
        ),
        "{v}"
    );
    assert_eq!(count(&s, "SELECT count(*) FROM changeset"), before);
}
