//! Tests of the review endpoints (specification 2, section 5.5): the open
//! changesets, one changeset, and the summaries of their messages.

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use iris_api::Auth;
use iris_changeset::{add_action, create, submit, Author, AuthorKind, Verb};
use iris_store::{Header, NewMessage, Store};
use serde_json::{json, Map, Value};
use tower::ServiceExt;

const NOW: i64 = 1_794_787_200_000;

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp(name: &str) -> TempDir {
    let dir = std::env::temp_dir().join(format!("iris-api-review-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    TempDir(dir)
}

fn agent() -> Author {
    Author {
        kind: AuthorKind::Agent,
        name: Some("Claude Code".into()),
        session: Some("se-1".into()),
        job: None,
    }
}

/// Messages 1 and 2 in thread 1, message 3 in thread 2, all in the inbox.
fn store(dir: &TempDir) -> Arc<Store> {
    let s = Store::open(&dir.0).unwrap();
    let a = s.add_account("gmail", "owner@example.com").unwrap();
    s.put_labels(a, &[("INBOX", "INBOX", "system")]).unwrap();
    let m = |id: &str, t: &str, subject: &str| NewMessage {
        provider_id: id.into(),
        thread_provider_id: t.into(),
        labels: vec!["INBOX".into()],
        header: Header {
            date: Some(NOW - 3_600_000),
            from_addr: Some("priya@example.com".into()),
            from_name: Some("Priya".into()),
            subject: Some(subject.into()),
            ..Default::default()
        },
        ..Default::default()
    };
    s.put_messages(
        a,
        &[
            m("g1", "gt1", "Q4 numbers"),
            m("g2", "gt1", "Re: Q4 numbers"),
            m("g3", "gt2", "News"),
        ],
    )
    .unwrap();
    Arc::new(s)
}

/// Submit a changeset with one action for each target list. Gives its ID.
fn changeset(s: &Store, why: &str, targets: &[&[&str]], at: i64) -> i64 {
    let cs = create(s, &agent(), at).unwrap();
    for t in targets {
        add_action(
            s,
            cs,
            &agent(),
            Verb::Archive,
            t,
            &Map::new(),
            Some("Old mail."),
        )
        .unwrap();
    }
    submit(s, cs, &agent(), why, at).unwrap();
    cs
}

async fn get(store: &Arc<Store>, uri: &str) -> (StatusCode, Value) {
    let auth = Arc::new(Auth::new("127.0.0.1:4800".parse().unwrap()).unwrap());
    let req = Request::get(uri)
        .header("host", "127.0.0.1:4800")
        .header("authorization", auth.bearer())
        .body(Body::empty())
        .unwrap();
    let res = iris_api::router(store.clone(), auth)
        .oneshot(req)
        .await
        .unwrap();
    let code = res.status();
    let body = to_bytes(res.into_body(), 64 << 20).await.unwrap();
    (code, serde_json::from_slice(&body).unwrap())
}

fn ids(v: &Value) -> Vec<&str> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|d| d["id"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn lists_each_open_changeset_newest_first() {
    let dir = temp("open");
    let s = store(&dir);
    let first = changeset(&s, "Clean up.", &[&["t-1"]], NOW);
    let second = changeset(&s, "The news.", &[&["m-3"]], NOW + 1000);
    // A draft is not open: the author has not submitted it.
    create(&s, &agent(), NOW).unwrap();
    // A changeset with no open action is closed.
    let closed = changeset(&s, "Nothing.", &[&["m-1"]], NOW + 2000);
    s.write(|tx| {
        tx.execute(
            "UPDATE changeset_action SET state = 'denied', decided_by = 'engine', reason = 'x', decided_at = ?2
             WHERE changeset_id = ?1",
            [closed, NOW + 2000],
        )?;
        Ok(())
    })
    .unwrap();

    let (code, v) = get(&s, "/api/changesets").await;
    assert_eq!(code, StatusCode::OK);
    let list = &v["changesets"];
    assert_eq!(ids(list), [format!("cs-{second}"), format!("cs-{first}")]);
    // Each action carries the real message IDs that it changes.
    assert_eq!(
        list[1]["actions"][0]["resolved"]["messages"],
        json!(["m-1", "m-2"])
    );
    assert_eq!(list[1]["actions"][0]["decision"]["state"], "proposed");
    assert_eq!(list[1]["why"], "Clean up.");

    // The brief counts the open changesets and the actions that wait for a person.
    let (_, b) = get(&s, "/api/brief").await;
    assert_eq!(b["review"], json!({ "changesets": 2, "actions": 2 }));
}

#[tokio::test]
async fn gives_one_changeset_by_its_id() {
    let dir = temp("one");
    let s = store(&dir);
    let cs = changeset(&s, "Clean up.", &[&["t-1"], &["m-3"]], NOW);
    let (code, v) = get(&s, &format!("/api/changesets/cs-{cs}")).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(v["id"], format!("cs-{cs}"));
    assert_eq!(v["actions"].as_array().unwrap().len(), 2);
    for missing in ["cs-99", "cs-0", "77", "m-1", "cs-1x", "cs-%2B1"] {
        assert_eq!(
            get(&s, &format!("/api/changesets/{missing}")).await.0,
            StatusCode::NOT_FOUND,
            "{missing}"
        );
    }
    // With no changeset, the list is empty.
    let dir = temp("none");
    let empty = store(&dir);
    assert_eq!(
        get(&empty, "/api/changesets").await.1,
        json!({ "changesets": [] })
    );
    assert_eq!(
        get(&empty, "/api/brief").await.1["review"],
        json!({ "changesets": 0, "actions": 0 })
    );
}

#[tokio::test]
async fn gives_the_summary_of_each_message_id_in_order() {
    let dir = temp("messages");
    let s = store(&dir);
    s.write(|tx| {
        tx.execute("UPDATE message SET gone_at = 1 WHERE id = 2", [])?;
        Ok(())
    })
    .unwrap();
    let (code, v) = get(&s, "/api/messages?ids=m-3,m-1,m-2,m-99").await;
    assert_eq!(code, StatusCode::OK);
    let m = &v["messages"];
    assert_eq!(ids(m), ["m-3", "m-1", "m-2", "m-99"]);
    assert_eq!(m[0]["subject"], "News");
    assert_eq!(m[1]["from"]["name"], "Priya");
    assert_eq!(m[1]["gone"], false);
    // A gone message and a message that is not in the mirror show only their ID.
    assert_eq!(m[2], json!({ "id": "m-2", "gone": true }));
    assert_eq!(m[3], json!({ "id": "m-99", "gone": true }));

    let many = (1..=201)
        .map(|i| format!("m-{i}"))
        .collect::<Vec<_>>()
        .join(",");
    for bad in [
        "",
        "ids=",
        "ids=t-1",
        "ids=m-1,,m-2",
        "ids=m-1,x",
        &format!("ids={many}"),
    ] {
        assert_eq!(
            get(&s, &format!("/api/messages?{bad}")).await.0,
            StatusCode::BAD_REQUEST,
            "{bad}"
        );
    }
    let most = (1..=200)
        .map(|i| format!("m-{i}"))
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(
        get(&s, &format!("/api/messages?ids={most}")).await.1["messages"]
            .as_array()
            .unwrap()
            .len(),
        200
    );
}
