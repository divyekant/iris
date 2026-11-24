//! Tests of the review endpoints (specification 2, section 5.5): the open
//! changesets, one changeset, and the summaries of their messages.

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use iris_api::Auth;
use iris_changeset::{
    add_action, create, decide, revise, submit, ActionId, Author, AuthorKind, By, State, Verb,
};
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
    (code, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

async fn post(store: &Arc<Store>, uri: &str, body: &str) -> (StatusCode, Value) {
    let auth = Arc::new(Auth::new("127.0.0.1:4800".parse().unwrap()).unwrap());
    let req = Request::post(uri)
        .header("host", "127.0.0.1:4800")
        .header("authorization", auth.bearer())
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let res = iris_api::router(store.clone(), auth)
        .oneshot(req)
        .await
        .unwrap();
    let code = res.status();
    let body = to_bytes(res.into_body(), 64 << 20).await.unwrap();
    (code, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

/// The state of each action of a changeset, from its document.
async fn states(s: &Arc<Store>, cs: i64) -> Vec<String> {
    let (_, v) = get(s, &format!("/api/changesets/cs-{cs}")).await;
    v["actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["decision"]["state"].as_str().unwrap().to_string())
        .collect()
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

    // A new revision is a draft: the store refuses an approval until its
    // submit, so the list and the brief both leave the changeset out, though
    // its older actions are still proposed.
    revise(&s, first, &agent()).unwrap();
    let (_, v) = get(&s, "/api/changesets").await;
    assert_eq!(ids(&v["changesets"]), [format!("cs-{second}")]);
    assert_eq!(
        get(&s, "/api/brief").await.1["review"],
        json!({ "changesets": 1, "actions": 1 })
    );
    // After the submit of the new revision, it is open again, with only its new action.
    add_action(
        &s,
        first,
        &agent(),
        Verb::Archive,
        &["m-3"],
        &Map::new(),
        None,
    )
    .unwrap();
    submit(&s, first, &agent(), "Only the news.", NOW + 3000).unwrap();
    assert_eq!(
        get(&s, "/api/brief").await.1["review"],
        json!({ "changesets": 2, "actions": 2 })
    );
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

/// The ID of a message in the text, by the rule of specification 1,
/// section 5.7, written apart from the code under test.
fn oracle(segment: &str) -> Option<String> {
    let digits = segment.strip_prefix("m-")?;
    if digits.is_empty() || digits.starts_with('0') || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let row: u128 = digits.parse().ok()?;
    (row <= i64::MAX as u128).then(|| format!("m-{row}"))
}

/// Property: for any ID text, the answer is a 400 unless each part is a
/// message ID in its one canonical form (no zero in front) and there are
/// 200 parts or fewer. Else it has one entry for each part, in order.
#[test]
fn the_ids_parser_accepts_only_message_ids_and_keeps_their_order() {
    let dir = temp("ids-property");
    let s = store(&dir);
    let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut next = move |n: u64| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state % n
    };
    const PARTS: [&str; 14] = [
        "m-1",
        "m-2",
        "m-3",
        "m-007",
        "m-99",
        "m-0",
        "m-",
        "t-1",
        "c-1",
        "",
        " m-1",
        "m-+1",
        "m-١",
        "m-99999999999999999999",
    ];
    for _ in 0..3000 {
        let count = if next(10) == 0 {
            195 + next(10)
        } else {
            1 + next(6)
        };
        let parts: Vec<String> = (0..count)
            .map(|_| match next(4) {
                0 => format!("m-{}", next(1_000_000)),
                _ => PARTS[next(PARTS.len() as u64) as usize].to_string(),
            })
            .collect();
        let text = parts.join(",");
        let expected: Option<Vec<String>> = parts.iter().map(|p| oracle(p)).collect();
        let expected = expected.filter(|e| e.len() <= 200);
        match (iris_api::messages_by_id(&s, &text), expected) {
            (Ok(got), Some(want)) => {
                let got: Vec<&str> = got.iter().map(|m| m["id"].as_str().unwrap()).collect();
                assert_eq!(got, want, "{text:?}");
            }
            (Err(iris_api::Error::Query(_)), None) => {}
            (got, want) => panic!("{text:?}: {got:?} but expected {want:?}"),
        }
    }
}

#[tokio::test]
async fn approves_and_denies_the_actions_of_one_changeset_together() {
    let dir = temp("decide");
    let s = store(&dir);
    let cs = changeset(&s, "Clean up.", &[&["t-1"], &["m-3"]], NOW);
    let (_, doc) = get(&s, &format!("/api/changesets/cs-{cs}")).await;
    let hash = |i: usize| doc["actions"][i]["hash"].as_str().unwrap().to_string();
    let uri = format!("/api/changesets/cs-{cs}/decide");

    let (code, after) = post(
        &s,
        &uri,
        &json!({ "actions": [
            { "revision": 1, "n": 1, "approve": true, "hash": hash(0) },
            { "revision": 1, "n": 2, "approve": false },
        ] })
        .to_string(),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    // The answer is the changeset after the decision.
    assert_eq!(after["actions"][0]["decision"]["state"], "approved");
    assert_eq!(after["actions"][0]["decision"]["by"], "person");
    assert_eq!(after["actions"][1]["decision"]["state"], "denied");
    assert_eq!(after["actions"][1]["decision"]["reason"], iris_api::DENIED);
    assert_eq!(
        get(&s, "/api/brief").await.1["review"],
        json!({ "changesets": 0, "actions": 0 })
    );

    // A second decision on a decided action is a conflict, not an error of the engine.
    let again = json!({ "actions": [{ "revision": 1, "n": 2, "approve": false }] }).to_string();
    assert_eq!(post(&s, &uri, &again).await.0, StatusCode::CONFLICT);
}

#[tokio::test]
async fn a_refused_decision_writes_nothing() {
    let dir = temp("decide-refused");
    let s = store(&dir);
    let cs = changeset(&s, "Clean up.", &[&["t-1"], &["m-3"]], NOW);
    let uri = format!("/api/changesets/cs-{cs}/decide");
    // The deny comes first and is valid. The approval names a hash that the
    // review did not show, so the whole decision rolls back.
    let stale = json!({ "actions": [
        { "revision": 1, "n": 2, "approve": false },
        { "revision": 1, "n": 1, "approve": true, "hash": "0".repeat(64) },
    ] })
    .to_string();
    let (code, v) = post(&s, &uri, &stale).await;
    assert_eq!(code, StatusCode::CONFLICT, "{v}");
    assert_eq!(states(&s, cs).await, ["proposed", "proposed"]);
    // An action that is not in the changeset.
    let missing = json!({ "actions": [{ "revision": 1, "n": 9, "approve": false }] }).to_string();
    assert_eq!(post(&s, &uri, &missing).await.0, StatusCode::CONFLICT);
    // While a new revision is a draft, the store refuses an approval.
    let (_, doc) = get(&s, &format!("/api/changesets/cs-{cs}")).await;
    let hash = doc["actions"][0]["hash"].as_str().unwrap().to_string();
    revise(&s, cs, &agent()).unwrap();
    let draft = json!({ "actions": [{ "revision": 1, "n": 1, "approve": true, "hash": hash }] })
        .to_string();
    assert_eq!(post(&s, &uri, &draft).await.0, StatusCode::CONFLICT);
    // A denial too: the review hides a changeset while its new revision is a draft.
    let deny = json!({ "actions": [{ "revision": 1, "n": 2, "approve": false }] }).to_string();
    assert_eq!(post(&s, &uri, &deny).await.0, StatusCode::CONFLICT);
    assert_eq!(states(&s, cs).await, ["proposed", "proposed"]);
}

#[tokio::test]
async fn refuses_a_decision_that_is_not_valid() {
    let dir = temp("decide-bad");
    let s = store(&dir);
    let cs = changeset(&s, "Clean up.", &[&["t-1"]], NOW);
    let uri = format!("/api/changesets/cs-{cs}/decide");
    let many: Vec<Value> = (1..=501)
        .map(|n| json!({ "revision": 1, "n": n, "approve": false }))
        .collect();
    for bad in [
        "".to_string(),
        "{}".to_string(),
        json!({ "actions": [] }).to_string(),
        json!({ "actions": many }).to_string(),
        json!({ "actions": [{ "revision": 1, "n": 1, "approve": true }] }).to_string(),
        json!({ "actions": [{ "revision": 1, "n": 1, "approve": false }, { "revision": 1, "n": 1, "approve": false }] }).to_string(),
        json!({ "actions": [{ "revision": 1, "n": 1, "approve": false, "why": "x" }] }).to_string(),
        json!({ "actions": [{ "revision": "1", "n": 1, "approve": false }] }).to_string(),
    ] {
        assert_eq!(post(&s, &uri, &bad).await.0, StatusCode::BAD_REQUEST, "{bad}");
    }
    assert_eq!(states(&s, cs).await, ["proposed"]);
    let deny = json!({ "actions": [{ "revision": 1, "n": 1, "approve": false }] }).to_string();
    assert_eq!(
        post(&s, "/api/changesets/cs-99/decide", &deny).await.0,
        StatusCode::NOT_FOUND
    );
    // A decision needs a POST.
    assert_eq!(get(&s, &uri).await.0, StatusCode::METHOD_NOT_ALLOWED);
}

fn parse(id: &str) -> i64 {
    id.strip_prefix("cs-").unwrap().parse().unwrap()
}

/// What Iris handled without the person: each action that an `allow` rule
/// approved, with the message count and the reason, and no other action.
/// The brief count and the full list agree, page by page.
#[tokio::test]
async fn lists_each_action_that_a_rule_decided() {
    let dir = temp("handled");
    let s = store(&dir);
    s.write(|tx| {
        tx.execute_batch(
            "INSERT INTO policy_version(text, sha256, confirmed_at) VALUES ('a', 'h1', 0);
             INSERT INTO policy_rule_id(id) VALUES (1), (2);",
        )?;
        Ok(())
    })
    .unwrap();
    let rule = By::Rule { id: 2, policy: 1 };
    let at = |changeset, n| ActionId {
        changeset,
        revision: 1,
        n,
    };
    // Five changesets: actions 1 approved by the rule, 2 waits, 3 in review.
    let mut want = Vec::new();
    for i in 0..5 {
        let cs = changeset(
            &s,
            "Old mail.",
            &[&["m-1", "m-2"], &["m-3"], &["m-3"]],
            NOW + i,
        );
        decide(
            &s,
            at(cs, 1),
            State::Approved,
            rule,
            Some("Rule 2 allows it."),
            NOW + i,
        )
        .unwrap();
        decide(
            &s,
            at(cs, 3),
            State::InReview,
            By::Policy,
            Some("No rule."),
            NOW + i,
        )
        .unwrap();
        want.insert(0, format!("cs-{cs}"));
    }
    let (_, b) = get(&s, "/api/brief").await;
    assert_eq!(b["handled"], json!({"count": 10, "verbs": {"archive": 10}}));
    let mut got = Vec::new();
    let mut after = String::new();
    loop {
        let (code, p) = get(&s, &format!("/api/handled?limit=2{after}")).await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(p["count"], 5);
        for a in p["actions"].as_array().unwrap() {
            assert_eq!(a["args"], json!({}));
            assert_eq!(a["n"], 1);
            assert_eq!(a["verb"], "archive");
            assert_eq!(a["messages"], 2);
            assert_eq!(a["state"], "approved");
            assert_eq!(a["rule"], 2);
            assert_eq!(a["policy"], 1);
            assert_eq!(a["why"], "Old mail.");
            assert_eq!(a["reason"], "Rule 2 allows it.");
            got.push(a["changeset"].as_str().unwrap().to_string());
        }
        match p["next"].as_str() {
            Some(n) => after = format!("&after={n}"),
            None => break,
        }
    }
    assert_eq!(got, want);
    // A rule decision in each later state stays: a failed apply can have
    // changed some messages (spec 2 section 7.2). The moves after approved
    // need the ledger (R2-03), so the test writes the state with no trigger.
    let first = parse(&want[4]);
    s.write(|tx| {
        tx.execute_batch("DROP TRIGGER action_state")?;
        for (state, n) in [("failed", 1), ("undone", 2)] {
            tx.execute(
                "UPDATE changeset_action SET state = ?1
                     WHERE changeset_id = ?2 AND n = ?3",
                rusqlite::params![state, first + n, 1],
            )?;
        }
        // An approved action that the author withdraws gets the decision
        // of the author and changed nothing: not handled.
        tx.execute(
            "UPDATE changeset_action SET state = 'cancelled', decided_by = 'author',
               rule = NULL, policy = NULL
             WHERE changeset_id = ?1 AND n = 1",
            [first + 4],
        )?;
        // In review waits for the person: not handled.
        tx.execute(
            "UPDATE changeset_action SET state = 'in_review'
                 WHERE changeset_id = ?1 AND n = 1",
            [first + 3],
        )?;
        Ok(())
    })
    .unwrap();
    let (_, b) = get(&s, "/api/brief").await;
    assert_eq!(b["handled"], json!({"count": 6, "verbs": {"archive": 6}}));
    let (_, p) = get(&s, "/api/handled?limit=200").await;
    assert_eq!(p["count"], 3);
    let states: Vec<&str> = p["actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["state"].as_str().unwrap())
        .collect();
    assert_eq!(states, ["undone", "failed", "approved"]);
    for bad in [
        "/api/handled?after=x",
        "/api/handled?after=01",
        "/api/handled?limit=0",
    ] {
        assert_eq!(get(&s, bad).await.0, StatusCode::BAD_REQUEST, "{bad}");
    }
}

#[tokio::test]
async fn the_person_withdraws_an_open_changeset() {
    let dir = temp("withdraw");
    let s = store(&dir);
    let cs = changeset(&s, "Clean up.", &[&["t-1"], &["m-3"]], NOW);
    let (_, doc) = get(&s, &format!("/api/changesets/cs-{cs}")).await;
    let hash = doc["actions"][0]["hash"].as_str().unwrap().to_string();
    // One action approved (it waits for apply), one waits for the person.
    let approve = json!({ "actions": [{ "revision": 1, "n": 1, "approve": true, "hash": hash }] });
    let decide = format!("/api/changesets/cs-{cs}/decide");
    assert_eq!(
        post(&s, &decide, &approve.to_string()).await.0,
        StatusCode::OK
    );

    let uri = format!("/api/changesets/cs-{cs}/withdraw");
    let (code, after) = post(&s, &uri, "").await;
    assert_eq!(code, StatusCode::OK, "{after}");
    // The answer is the changeset after the withdraw: each open action is cancelled.
    assert_eq!(after["state"], "withdrawn");
    assert_eq!(states(&s, cs).await, ["cancelled", "cancelled"]);
    assert_eq!(get(&s, "/api/changesets").await.1["changesets"], json!([]));
    assert_eq!(
        get(&s, "/api/brief").await.1["review"],
        json!({ "changesets": 0, "actions": 0 })
    );
    // A second withdraw, or a decision after the withdraw, is a conflict.
    assert_eq!(post(&s, &uri, "").await.0, StatusCode::CONFLICT);
    let deny = json!({ "actions": [{ "revision": 1, "n": 2, "approve": false }] }).to_string();
    assert_eq!(post(&s, &decide, &deny).await.0, StatusCode::CONFLICT);
}

#[tokio::test]
async fn withdraws_each_changeset_once() {
    let dir = temp("withdraw-states");
    let s = store(&dir);
    // A changeset with a new revision that is a draft: the open action and the draft action are cancelled.
    let draft = changeset(&s, "Clean up.", &[&["t-1"]], NOW);
    revise(&s, draft, &agent()).unwrap();
    add_action(
        &s,
        draft,
        &agent(),
        Verb::Archive,
        &["m-3"],
        &Map::new(),
        None,
    )
    .unwrap();
    let uri = format!("/api/changesets/cs-{draft}/withdraw");
    let (code, after) = post(&s, &uri, "").await;
    assert_eq!(code, StatusCode::OK, "{after}");
    assert_eq!(after["state"], "withdrawn");
    let all: Vec<&str> = after["actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["decision"]["state"].as_str().unwrap_or("draft"))
        .collect();
    assert_eq!(all, ["cancelled", "cancelled"], "{after}");

    // A closed changeset: the person denied its only action. The denial stays.
    let closed = changeset(&s, "Read.", &[&["m-3"]], NOW);
    let deny = json!({ "actions": [{ "revision": 1, "n": 1, "approve": false }] }).to_string();
    assert_eq!(
        post(&s, &format!("/api/changesets/cs-{closed}/decide"), &deny)
            .await
            .0,
        StatusCode::OK
    );
    let (code, after) = post(&s, &format!("/api/changesets/cs-{closed}/withdraw"), "").await;
    assert_eq!(code, StatusCode::OK, "{after}");
    assert_eq!(after["state"], "withdrawn");
    assert_eq!(states(&s, closed).await, ["denied"]);

    assert_eq!(post(&s, &uri, "").await.0, StatusCode::CONFLICT);
    assert_eq!(
        post(&s, "/api/changesets/cs-99/withdraw", "").await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(get(&s, &uri).await.0, StatusCode::METHOD_NOT_ALLOWED);
}

/// Review comments (docs/specs/2026-11-17-review-comments.md, section 3).
#[tokio::test]
async fn the_person_comments_on_a_changeset_of_a_job() {
    let dir = temp("comment");
    let s = store(&dir);
    s.write(|tx| Ok(tx.execute("UPDATE message SET understood_at = 1", [])?))
        .unwrap();
    let j = iris_job::create(&s, "Tidy", None, NOW).unwrap();
    iris_job::add_scope(&s, j, iris_job::Ref::Thread(1), iris_job::By::Person, NOW).unwrap();
    let who = Author {
        job: Some(iris_job::format_job_id(j)),
        ..agent()
    };
    let cs = create(&s, &who, NOW).unwrap();
    add_action(&s, cs, &who, Verb::Archive, &["m-1"], &Map::new(), None).unwrap();
    submit(&s, cs, &who, "Archive.", NOW).unwrap();
    let uri = format!("/api/changesets/cs-{cs}/comments");
    let body = json!({"revision": 1, "text": "Keep it.", "key": "k-0123456789abcdef"}).to_string();

    let (code, first) = post(&s, &uri, &body).await;
    assert_eq!(code, StatusCode::OK, "{first}");
    assert_eq!(
        (&first["changeset"], &first["revision"], &first["text"]),
        (&json!(format!("cs-{cs}")), &json!(1), &json!("Keep it."))
    );
    // A retry after a lost answer gives the same comment.
    assert_eq!(post(&s, &uri, &body).await, (StatusCode::OK, first.clone()));

    // A bad body is a 400, a refused state a 409, an unknown changeset a 404.
    let bad = json!({"revision": 1, "text": " ", "key": "k-1123456789abcdef"}).to_string();
    assert_eq!(post(&s, &uri, &bad).await.0, StatusCode::BAD_REQUEST);
    let old = json!({"revision": 2, "text": "x", "key": "k-2123456789abcdef"}).to_string();
    assert_eq!(post(&s, &uri, &old).await.0, StatusCode::CONFLICT);
    let host = changeset(&s, "Archive.", &[&["m-3"]], NOW);
    let other = format!("/api/changesets/cs-{host}/comments");
    let ok = json!({"revision": 1, "text": "x", "key": "k-3123456789abcdef"}).to_string();
    assert_eq!(post(&s, &other, &ok).await.0, StatusCode::CONFLICT);
    assert_eq!(
        post(&s, "/api/changesets/cs-99/comments", &ok).await.0,
        StatusCode::NOT_FOUND
    );

    assert_eq!(
        get(&s, &uri).await,
        (StatusCode::OK, json!({"comments": [first]}))
    );
    assert_eq!(
        get(&s, &other).await,
        (StatusCode::OK, json!({"comments": []}))
    );
}

/// Each submitted revision keeps its own reason (spec 2 section 5.6, issue 189).
#[tokio::test]
async fn gives_the_reason_of_each_submitted_revision() {
    let dir = temp("revisions");
    let s = store(&dir);
    let cs = changeset(&s, "Old newsletters.", &[&["m-1"]], 1_000);
    revise(&s, cs, &agent()).unwrap();
    add_action(
        &s,
        cs,
        &agent(),
        Verb::Archive,
        &["m-2"],
        &Map::new(),
        Some("Old mail."),
    )
    .unwrap();
    let uri = format!("/api/changesets/cs-{cs}/revisions");
    // The draft revision 2 has no reason yet.
    assert_eq!(
        get(&s, &uri).await,
        (
            StatusCode::OK,
            json!({"revisions": [{"revision": 1, "why": "Old newsletters.", "submitted": "1970-01-01T00:00:01Z"}]})
        )
    );
    submit(&s, cs, &agent(), "Also the thread of Q4.", 2_000).unwrap();
    assert_eq!(
        get(&s, &uri).await.1["revisions"],
        json!([
            {"revision": 1, "why": "Old newsletters.", "submitted": "1970-01-01T00:00:01Z"},
            {"revision": 2, "why": "Also the thread of Q4.", "submitted": "1970-01-01T00:00:02Z"},
        ])
    );
    assert_eq!(
        get(&s, "/api/changesets/cs-99/revisions").await.0,
        StatusCode::NOT_FOUND
    );
}
