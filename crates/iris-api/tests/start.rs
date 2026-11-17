//! Tests of `POST /api/jobs` and `POST /api/policy/grant`
//! (`docs/specs/2026-11-17-start-job.md`): a job from one item, with the
//! correct mail, and a grant inside one job.

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use iris_api::Auth;
use iris_job::Ref;
use iris_store::Store;
use serde_json::{json, Value};
use tower::ServiceExt;

/// After the real time of each start of the test.
const NOW: i64 = 4_000_000_000_000;

struct Dir(PathBuf, Arc<Store>);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Threads 1 and 2 hold the sources of commitment 1 (messages 1 and 2).
/// Thread 3 has no item of the job. Commitment 2 has a hostile source
/// (message 4). Conversation 3 is thread 1. Item 4 is a record. Commitment 5
/// is gone. Commitment 6 has a source that is not understood (message 5).
/// Commitment 9 shares its source (message 8) with signal 10.
/// Commitment 11 is done, conversation 12 is muted, conversation 13 is
/// quiet; each has the source message 1.
fn open(name: &str) -> Dir {
    let dir = std::env::temp_dir().join(format!("iris-api-start-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let s = Store::open(&dir).unwrap();
    s.write(|tx| {
        tx.execute_batch(
            r#"
INSERT INTO account (id, provider, address) VALUES (1, 'gmail', 'me@example.com');
INSERT INTO thread (id, account_id, provider_id) VALUES (1, 1, 'a'), (2, 1, 'b'), (3, 1, 'c');
INSERT INTO sender (address, name, kind) VALUES ('dana@example.com', 'Dana', 'person');
INSERT INTO message (id, account_id, thread_id, provider_id, internal_date, understood_at, hostile) VALUES
  (1, 1, 1, 'm1', 1000, 1, NULL),
  (2, 1, 2, 'm2', 2000, 1, NULL),
  (3, 1, 3, 'm3', 3000, 1, NULL),
  (4, 1, 3, 'm4', 4000, 1, '["lookalike"]'),
  (5, 1, 3, 'm5', 5000, NULL, NULL),
  (6, 1, 1, 'm6', 6000, 1, NULL),
  (8, 1, 3, 'm8', 8000, 1, NULL);
INSERT INTO header (message_id, date, from_addr, subject) VALUES
  (1, 1000, 'dana@example.com', 'Contract'), (2, 2000, 'dana@example.com', 'Section 4'),
  (3, 3000, 'dana@example.com', 'News'), (4, 4000, 'dana@example.com', 'Bad'),
  (5, 5000, 'dana@example.com', 'New'), (6, 6000, 'dana@example.com', 'Re: Contract'),
  (8, 8000, 'dana@example.com', 'Your code');
INSERT INTO item (id, form, account_id, status, doc, created_at, updated_at) VALUES
  (1, 'commitment', 1, 'open', '{}', 1, 1),
  (2, 'commitment', 1, 'open', '{}', 1, 1),
  (3, 'conversation', 1, 'active', '{}', 1, 1),
  (4, 'record', 1, 'open', '{}', 1, 1),
  (5, 'commitment', 1, 'gone', '{}', 1, 1),
  (6, 'commitment', 1, 'open', '{}', 1, 1),
  (7, 'commitment', 1, 'open', '{}', 1, 1),
  (9, 'commitment', 1, 'open', '{}', 1, 1),
  (10, 'signal', 1, 'open', '{}', 1, 1),
  (11, 'commitment', 1, 'done', '{}', 1, 1),
  (12, 'conversation', 1, 'muted', '{}', 1, 1),
  (13, 'conversation', 1, 'quiet', '{}', 1, 1);
INSERT INTO item_source (item_id, message_id, role) VALUES
  (1, 1, 'origin'), (1, 2, 'update'), (2, 3, 'origin'), (2, 4, 'evidence'),
  (3, 1, 'origin'), (3, 6, 'update'), (4, 3, 'origin'), (5, 1, 'origin'), (6, 5, 'origin'),
  (9, 8, 'origin'), (10, 8, 'origin'), (11, 1, 'origin'), (12, 1, 'origin'),
  (13, 1, 'origin');
"#,
        )?;
        Ok(())
    })
    .unwrap();
    Dir(dir, Arc::new(s))
}

impl Dir {
    async fn get(&self, uri: &str) -> (StatusCode, Value) {
        let auth = Arc::new(Auth::new("127.0.0.1:4800".parse().unwrap()).unwrap());
        let req = Request::get(uri)
            .header("host", "127.0.0.1:4800")
            .header("authorization", auth.bearer())
            .body(Body::empty())
            .unwrap();
        let res = iris_api::router(self.1.clone(), auth)
            .oneshot(req)
            .await
            .unwrap();
        let code = res.status();
        let bytes = to_bytes(res.into_body(), 1 << 20).await.unwrap();
        (code, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    /// The message IDs of the preview of a start from `item`.
    async fn preview(&self, item: &str) -> Vec<String> {
        let (code, v) = self.get(&format!("/api/items/{item}/job")).await;
        if code != StatusCode::OK {
            return Vec::new();
        }
        v["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["id"].as_str().unwrap().to_string())
            .collect()
    }

    async fn post(&self, uri: &str, body: Value) -> (StatusCode, Value) {
        let auth = Arc::new(Auth::new("127.0.0.1:4800".parse().unwrap()).unwrap());
        let req = Request::post(uri)
            .header("host", "127.0.0.1:4800")
            .header("authorization", auth.bearer())
            .header("content-type", "application/json")
            .body(Body::from(body.to_string()))
            .unwrap();
        let res = iris_api::router(self.1.clone(), auth)
            .oneshot(req)
            .await
            .unwrap();
        let code = res.status();
        let bytes = to_bytes(res.into_body(), 1 << 20).await.unwrap();
        (code, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    async fn start(&self, item: &str) -> (StatusCode, Value) {
        let messages = self.preview(item).await;
        self.post(
            "/api/jobs",
            json!({"item": item, "goal": "  Sign the contract.  ",
                   "due": "2026-11-20T17:00:00Z", "messages": messages}),
        )
        .await
    }

    fn jobs(&self) -> i64 {
        self.1
            .read(|c| Ok(c.query_row("SELECT count(*) FROM job", [], |r| r.get(0))?))
            .unwrap()
    }

    fn scope(&self, job: &str) -> Vec<Ref> {
        iris_job::scope(&self.1, iris_job::parse_job_id(job).unwrap()).unwrap()
    }
}

#[tokio::test]
async fn a_commitment_starts_a_job_with_the_item_and_the_threads_of_its_sources() {
    let d = open("commitment");
    let (code, job) = d.start("c-1").await;
    assert_eq!(code, StatusCode::CREATED, "{job}");
    assert_eq!(job["id"], json!("j-1"));
    assert_eq!(job["goal"], json!("Sign the contract."));
    assert_eq!(job["state"], json!("working"));
    assert_eq!(job["due"], json!("2026-11-20T17:00:00.000Z"));
    // Not thread 3, which holds no source of the item.
    let mut scope = d.scope("j-1");
    scope.sort();
    assert_eq!(scope, vec![Ref::Thread(1), Ref::Thread(2), Ref::Item(1)]);
    // Each ref is the person's.
    let by: Vec<String> =
        d.1.read(|c| {
            let mut q = c.prepare("SELECT DISTINCT by FROM job_scope")?;
            let v = q
                .query_map([], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            Ok(v)
        })
        .unwrap();
    assert_eq!(by, vec!["person".to_string()]);
}

#[tokio::test]
async fn a_conversation_starts_a_job_with_its_thread() {
    let d = open("conversation");
    let (code, job) = d
        .post(
            "/api/jobs",
            json!({"item": "v-3", "goal": "Reply to Dana.", "messages": d.preview("v-3").await}),
        )
        .await;
    assert_eq!(code, StatusCode::CREATED, "{job}");
    assert_eq!(job.get("due"), None);
    let mut scope = d.scope("j-1");
    scope.sort();
    assert_eq!(scope, vec![Ref::Thread(1), Ref::Item(3)]);
}

#[tokio::test]
async fn a_second_start_finds_the_open_job() {
    let d = open("again");
    assert_eq!(d.start("c-1").await.0, StatusCode::CREATED);
    let (code, v) = d.start("c-1").await;
    assert_eq!(code, StatusCode::CONFLICT, "{v}");
    assert_eq!(v["job"], json!("j-1"));
    assert_eq!(d.jobs(), 1);

    // A retry after the item closed still finds the job.
    d.1.write(|tx| {
        tx.execute("UPDATE item SET status = 'done' WHERE id = 1", [])?;
        Ok(())
    })
    .unwrap();
    let (code, v) = d
        .post(
            "/api/jobs",
            json!({"item": "c-1", "goal": "Go.", "messages": []}),
        )
        .await;
    assert_eq!(
        (code, &v["job"]),
        (StatusCode::CONFLICT, &json!("j-1")),
        "{v}"
    );
    d.1.write(|tx| {
        tx.execute("UPDATE item SET status = 'open' WHERE id = 1", [])?;
        Ok(())
    })
    .unwrap();

    // After the end of that job, a new job can start.
    iris_job::finish(
        &d.1,
        1,
        iris_job::By::Person,
        iris_job::End::Cancel,
        None,
        NOW,
    )
    .unwrap();
    let (code, v) = d.start("c-1").await;
    assert_eq!(code, StatusCode::CREATED, "{v}");
    assert_eq!(v["id"], json!("j-2"));
    // A revoked item ref is not a job of the item.
    iris_job::revoke_scope(&d.1, 2, Ref::Item(1), iris_job::By::Person, NOW).unwrap();
    assert_eq!(d.start("c-1").await.0, StatusCode::CREATED);
}

#[tokio::test]
async fn no_job_starts_from_an_item_that_an_agent_cannot_read() {
    let d = open("refused");
    for (item, code) in [
        ("c-2", StatusCode::CONFLICT),    // a hostile source
        ("c-5", StatusCode::CONFLICT),    // gone
        ("c-6", StatusCode::CONFLICT),    // a source that is not understood
        ("c-7", StatusCode::CONFLICT),    // no source
        ("c-9", StatusCode::CONFLICT),    // the source of a signal
        ("c-11", StatusCode::CONFLICT),   // done
        ("v-12", StatusCode::CONFLICT),   // muted
        ("r-4", StatusCode::BAD_REQUEST), // a record
        ("m-1", StatusCode::BAD_REQUEST),
        ("c-0", StatusCode::BAD_REQUEST),
        ("contract", StatusCode::BAD_REQUEST),
        ("c-99", StatusCode::NOT_FOUND),
        ("v-1", StatusCode::NOT_FOUND), // item 1 is a commitment
    ] {
        let (got, v) = d.start(item).await;
        assert_eq!(got, code, "{item}: {v}");
    }
    assert_eq!(d.jobs(), 0);
}

#[tokio::test]
async fn the_goal_the_due_date_and_the_messages_are_checked() {
    let d = open("checks");
    let m = json!(d.preview("c-1").await);
    for body in [
        json!({"item": "c-1", "goal": "   ", "messages": m}),
        json!({"item": "c-1", "goal": "x".repeat(2001), "messages": m}),
        json!({"item": "c-1", "goal": "Go.", "due": "2026-11-20", "messages": m}),
        json!({"item": "c-1", "goal": "Go.", "due": 5, "messages": m}),
        json!({"item": "c-1", "goal": "Go."}),
        json!({"item": "c-1", "messages": m}),
        json!({"item": "c-1", "goal": "Go.", "messages": ["t-1"]}),
        json!({"item": "c-1", "goal": "Go.", "messages": m, "scope": ["t-3"]}),
    ] {
        let (code, v) = d.post("/api/jobs", body.clone()).await;
        assert_eq!(code, StatusCode::BAD_REQUEST, "{body}: {v}");
    }
    assert_eq!(d.jobs(), 0);
    // 2,000 characters that are not ASCII are a valid goal.
    let (code, v) = d
        .post(
            "/api/jobs",
            json!({"item": "c-1", "goal": "é".repeat(2000), "messages": m}),
        )
        .await;
    assert_eq!(code, StatusCode::CREATED, "{v}");
}

#[tokio::test]
async fn the_preview_shows_each_message_of_the_grant() {
    let d = open("preview");
    let (code, v) = d.get("/api/items/c-1/job").await;
    assert_eq!(code, StatusCode::OK, "{v}");
    let rows: Vec<(String, String, bool)> = v["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            (
                m["id"].as_str().unwrap().into(),
                m["thread"].as_str().unwrap().into(),
                m["readable"].as_bool().unwrap(),
            )
        })
        .collect();
    // Message 6 is not a source, but its thread grants it. Thread 3 is not
    // in the grant.
    let want = [("m-1", "t-1"), ("m-6", "t-1"), ("m-2", "t-2")];
    assert_eq!(
        rows,
        want.map(|(m, t)| (m.to_string(), t.to_string(), true))
            .to_vec()
    );
    assert_eq!(v["messages"][0]["subject"], json!("Contract"));

    // The preview shows mail that an agent cannot read now too.
    d.1.write(|tx| {
        tx.execute_batch(
            "INSERT INTO message (id, account_id, thread_id, provider_id, internal_date)
             VALUES (7, 1, 2, 'm7', 7000);",
        )?;
        Ok(())
    })
    .unwrap();
    let (_, v) = d.get("/api/items/c-1/job").await;
    assert_eq!(v["messages"][3]["id"], json!("m-7"));
    assert_eq!(v["messages"][3]["readable"], json!(false));

    // The same checks as the start.
    for (item, code) in [
        ("c-2", StatusCode::CONFLICT),
        ("r-4", StatusCode::BAD_REQUEST),
        ("c-99", StatusCode::NOT_FOUND),
    ] {
        assert_eq!(
            d.get(&format!("/api/items/{item}/job")).await.0,
            code,
            "{item}"
        );
    }
}

#[tokio::test]
async fn a_start_grants_only_the_mail_that_the_person_saw() {
    let d = open("saw");
    let seen = d.preview("c-1").await;
    // New mail in thread 2 after the preview.
    d.1.write(|tx| {
        tx.execute_batch(
            "INSERT INTO message (id, account_id, thread_id, provider_id, internal_date, understood_at)
             VALUES (7, 1, 2, 'm7', 7000, 1);",
        )?;
        Ok(())
    })
    .unwrap();
    let goal = "Sign the contract.";
    let (code, v) = d
        .post(
            "/api/jobs",
            json!({"item": "c-1", "goal": goal, "messages": seen}),
        )
        .await;
    assert_eq!(code, StatusCode::CONFLICT, "{v}");
    // A list with one message less, or one message more.
    let now = d.preview("c-1").await;
    for list in [&now[1..], &[now.as_slice(), &["m-3".to_string()]].concat()] {
        let (code, v) = d
            .post(
                "/api/jobs",
                json!({"item": "c-1", "goal": goal, "messages": list}),
            )
            .await;
        assert_eq!(code, StatusCode::CONFLICT, "{list:?}: {v}");
    }
    assert_eq!(d.jobs(), 0);
    let (code, v) = d
        .post(
            "/api/jobs",
            json!({"item": "c-1", "goal": goal, "messages": now}),
        )
        .await;
    assert_eq!(code, StatusCode::CREATED, "{v}");
}

#[tokio::test]
async fn a_grant_writes_one_rule_for_an_open_job() {
    let d = open("grant");
    d.start("c-1").await;
    let (code, p) = d
        .post(
            "/api/policy/grant",
            json!({"job": "j-1", "verbs": ["archive", "label"], "version": null}),
        )
        .await;
    assert_eq!(code, StatusCode::OK, "{p}");
    let rule = &p["rules"][0];
    assert_eq!(rule["job"], json!("j-1"));
    assert_eq!(rule["effect"], json!("allow"));
    assert_eq!(rule["verbs"], json!(["archive", "label"]));
    assert_eq!(rule["when"]["author"], json!("agent"));
    let file = d.0.join(iris_policy::FILE);
    let text = std::fs::read_to_string(&file).unwrap();

    let version = p["version"].clone();
    for (body, code) in [
        (
            json!({"job": "j-1", "verbs": ["send"], "version": version}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"job": "j-1", "verbs": ["draft"], "version": version}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"job": "j-1", "verbs": [], "version": version}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"job": "j-1", "verbs": ["archive", "archive"], "version": version}),
            StatusCode::BAD_REQUEST,
        ),
        (
            json!({"job": "j-9", "verbs": ["archive"], "version": version}),
            StatusCode::NOT_FOUND,
        ),
        (
            json!({"job": "J-1", "verbs": ["archive"], "version": version}),
            StatusCode::NOT_FOUND,
        ),
        (
            json!({"job": "j-1", "verbs": ["archive"], "version": null}),
            StatusCode::CONFLICT,
        ),
    ] {
        let (got, v) = d.post("/api/policy/grant", body.clone()).await;
        assert_eq!(got, code, "{body}: {v}");
    }
    // A done job gets no grant.
    iris_job::finish(
        &d.1,
        1,
        iris_job::By::Person,
        iris_job::End::Cancel,
        None,
        NOW,
    )
    .unwrap();
    let (got, v) = d
        .post(
            "/api/policy/grant",
            json!({"job": "j-1", "verbs": ["read"], "version": version}),
        )
        .await;
    assert_eq!(got, StatusCode::CONFLICT, "{v}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), text);
}

#[tokio::test]
async fn an_item_that_closed_after_the_preview_starts_no_job() {
    let d = open("closed");
    let seen = d.preview("c-1").await;
    d.1.write(|tx| {
        tx.execute("UPDATE item SET status = 'done' WHERE id = 1", [])?;
        Ok(())
    })
    .unwrap();
    let (code, v) = d
        .post(
            "/api/jobs",
            json!({"item": "c-1", "goal": "Go.", "messages": seen}),
        )
        .await;
    assert_eq!(code, StatusCode::CONFLICT, "{v}");
    assert_eq!(d.jobs(), 0);
    // A quiet conversation can start a job.
    assert_eq!(d.start("v-13").await.0, StatusCode::CREATED);
}
