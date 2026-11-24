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
    // The full job document, with the create event of the person.
    let errors: Vec<String> = iris_spec::validator("job")
        .iter_errors(&job)
        .map(|e| e.to_string())
        .collect();
    assert!(errors.is_empty(), "{errors:?}: {job}");
    assert_eq!(job["events"][0]["by"], json!("person"));
    // The same document as the `job` field of the job page (issue #398).
    let (code, page) = d.get("/api/jobs/j-1").await;
    assert_eq!(code, StatusCode::OK, "{page}");
    assert_eq!(page["job"], job);
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
    // Only an item of the same kind finds the job: no conversation 1 exists.
    for item in ["v-1", "c-3"] {
        let (code, v) = d
            .post(
                "/api/jobs",
                json!({"item": item, "goal": "Go.", "messages": []}),
            )
            .await;
        assert_ne!(v.get("job"), Some(&json!("j-1")), "{item}: {v}");
        assert_ne!(code, StatusCode::CREATED, "{item}: {v}");
    }

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
        // The job table holds no time before 1970 or after 9999, and no NUL.
        json!({"item": "c-1", "goal": "Go.", "due": "1969-12-31T23:59:59Z", "messages": m}),
        json!({"item": "c-1", "goal": "Go.", "due": "9999-12-31T23:59:59-01:00", "messages": m}),
        json!({"item": "c-1", "goal": "Go\u{0}.", "messages": m}),
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

    // The order is the date that the summary shows: the header date first.
    d.1.write(|tx| {
        tx.execute_batch(
            "INSERT INTO message (id, account_id, thread_id, provider_id, internal_date, understood_at)
             VALUES (14, 1, 1, 'm14', 500, 1);
             INSERT INTO header (message_id, date, from_addr, subject)
             VALUES (14, 9000, 'dana@example.com', 'Late');",
        )?;
        Ok(())
    })
    .unwrap();
    let (_, v) = d.get("/api/items/c-1/job").await;
    let dates: Vec<&str> = v["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["thread"] == json!("t-1"))
        .map(|m| m["date"].as_str().unwrap())
        .collect();
    let mut sorted = dates.clone();
    sorted.sort();
    assert_eq!(dates, sorted);

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
            json!({"job": "j-1", "verbs": ["draft", "archive"], "version": version}),
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

/// Decision D25: a job from a typed goal, with no mail. A retry with the
/// same request ID makes no second job.
#[tokio::test]
async fn a_typed_goal_starts_a_job_with_no_mail_once_for_each_request() {
    let d = open("typed");
    let body =
        json!({"goal": " Book the dentist. ", "due": "2026-11-20T17:00:00Z", "request": "r-1"});
    let (code, job) = d.post("/api/jobs", body.clone()).await;
    assert_eq!(code, StatusCode::CREATED, "{job}");
    assert_eq!(job["goal"], json!("Book the dentist."));
    assert!(d.scope("j-1").is_empty());
    let (code, again) = d.post("/api/jobs", body).await;
    assert_eq!((code, &again["job"]), (StatusCode::CONFLICT, &json!("j-1")));
    let (code, other) = d
        .post(
            "/api/jobs",
            json!({"goal": "Book the dentist.", "request": "r-2"}),
        )
        .await;
    assert_eq!((code, &other["id"]), (StatusCode::CREATED, &json!("j-2")));
    // No request, an item with a request, messages with no item, or a bad
    // request ID: 400.
    for bad in [
        json!({"goal": "A."}),
        json!({"goal": "A.", "request": "r-3", "item": "c-1", "messages": []}),
        json!({"goal": "A.", "request": "r-3", "messages": []}),
        json!({"goal": "A.", "request": " "}),
        json!({"goal": "A.", "request": "x".repeat(65)}),
        json!({"goal": " ", "request": "r-3"}),
        json!({"goal": "A.", "due": "soon", "request": "r-3"}),
    ] {
        let (code, v) = d.post("/api/jobs", bad.clone()).await;
        assert_eq!(code, StatusCode::BAD_REQUEST, "{bad}: {v}");
    }
}

/// Decision D25: the person edits the goal and the due date of an open job.
#[tokio::test]
async fn the_person_edits_the_goal_and_the_due_date() {
    let d = open("edit");
    d.post("/api/jobs", json!({"goal": "A.", "request": "r-1"}))
        .await;
    let (code, job) = d
        .post(
            "/api/jobs/j-1/edit",
            json!({"goal": " B. ", "due": "2026-12-01T00:00:00Z"}),
        )
        .await;
    assert_eq!(code, StatusCode::OK, "{job}");
    assert_eq!(
        (&job["goal"], &job["due"]),
        (&json!("B."), &json!("2026-12-01T00:00:00.000Z"))
    );
    let (code, job) = d
        .post("/api/jobs/j-1/edit", json!({"goal": "B.", "due": null}))
        .await;
    assert_eq!(code, StatusCode::OK, "{job}");
    assert_eq!(job.get("due"), None);
    // The same values: 409. No due key, a bad goal: 400. No job: 404.
    let (code, _) = d
        .post("/api/jobs/j-1/edit", json!({"goal": "B.", "due": null}))
        .await;
    assert_eq!(code, StatusCode::CONFLICT);
    let (code, _) = d.post("/api/jobs/j-1/edit", json!({"goal": "C."})).await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    let (code, _) = d
        .post("/api/jobs/j-1/edit", json!({"goal": "", "due": null}))
        .await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    let (code, _) = d
        .post("/api/jobs/j-9/edit", json!({"goal": "C.", "due": null}))
        .await;
    assert_eq!(code, StatusCode::NOT_FOUND);
    // A done job: 409.
    iris_job::finish(
        &d.1,
        1,
        iris_job::By::Person,
        iris_job::End::Complete,
        None,
        NOW,
    )
    .unwrap();
    let (code, _) = d
        .post("/api/jobs/j-1/edit", json!({"goal": "C.", "due": null}))
        .await;
    assert_eq!(code, StatusCode::CONFLICT);
}

/// Decision D25: the person adds a thread from the preview that they saw,
/// and takes a ref out of the scope.
#[tokio::test]
async fn the_person_adds_a_thread_and_removes_a_ref() {
    let d = open("scope");
    d.post("/api/jobs", json!({"goal": "A.", "request": "r-1"}))
        .await;
    let (code, preview) = d.get("/api/jobs/j-1/scope/t-3").await;
    assert_eq!(code, StatusCode::OK, "{preview}");
    let ids: Vec<String> = preview["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(ids, ["m-3", "m-4", "m-5", "m-8"]);
    // Message 4 is hostile: the preview marks it.
    assert_eq!(preview["messages"][1]["readable"], json!(false));
    // A preview that misses a message: 409, and nothing changes.
    let (code, _) = d
        .post(
            "/api/jobs/j-1/scope",
            json!({"thread": "t-3", "messages": &ids[..3]}),
        )
        .await;
    assert_eq!(code, StatusCode::CONFLICT);
    assert!(d.scope("j-1").is_empty());
    let (code, page) = d
        .post(
            "/api/jobs/j-1/scope",
            json!({"thread": "t-3", "messages": ids}),
        )
        .await;
    assert_eq!(code, StatusCode::OK, "{page}");
    assert_eq!(d.scope("j-1"), vec![Ref::Thread(3)]);
    assert_eq!(
        page["scope"],
        json!([{"id": "t-3", "kind": "thread", "label": "News", "messages": 4}])
    );
    // No thread, no job: 404. Not a thread ID: 400.
    let (code, _) = d.get("/api/jobs/j-1/scope/t-99").await;
    assert_eq!(code, StatusCode::NOT_FOUND);
    let (code, _) = d.get("/api/jobs/j-9/scope/t-3").await;
    assert_eq!(code, StatusCode::NOT_FOUND);
    let (code, _) = d.get("/api/jobs/j-1/scope/m-3").await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    // Remove: a ref that is not in the scope is 404; the thread goes.
    let (code, _) = d
        .post("/api/jobs/j-1/scope/remove", json!({"ref": "t-2"}))
        .await;
    assert_eq!(code, StatusCode::NOT_FOUND);
    let (code, page) = d
        .post("/api/jobs/j-1/scope/remove", json!({"ref": "t-3"}))
        .await;
    assert_eq!(code, StatusCode::OK, "{page}");
    assert_eq!(page["scope"], json!([]));
    assert!(d.scope("j-1").is_empty());
    // An item ref of a job from an item: the ID names the form.
    let (code, _) = d.start("c-1").await;
    assert_eq!(code, StatusCode::CREATED);
    let (code, _) = d
        .post("/api/jobs/j-2/scope/remove", json!({"ref": "r-1"}))
        .await;
    assert_eq!(code, StatusCode::NOT_FOUND);
    let (code, _) = d
        .post("/api/jobs/j-2/scope/remove", json!({"ref": "c-1"}))
        .await;
    assert_eq!(code, StatusCode::OK);
    // A done job: no preview and no add.
    iris_job::finish(
        &d.1,
        2,
        iris_job::By::Person,
        iris_job::End::Cancel,
        None,
        NOW,
    )
    .unwrap();
    let (code, _) = d.get("/api/jobs/j-2/scope/t-3").await;
    assert_eq!(code, StatusCode::CONFLICT);
}

/// Decision D25: the scope of a done job never changes, and an item ref
/// keeps its ID after a later understanding deletes the item row.
#[tokio::test]
async fn a_done_job_keeps_its_scope_and_a_gone_item_keeps_its_id() {
    let d = open("scope-done");
    let (code, _) = d.start("c-1").await;
    assert_eq!(code, StatusCode::CREATED);
    let before = d.scope("j-1");
    assert!(before.contains(&Ref::Item(1)), "{before:?}");
    // The understanding deletes the item row: the ref keeps the ID `c-1`.
    d.1.write(|tx| Ok(tx.execute("DELETE FROM item WHERE id = 1", [])?))
        .unwrap();
    let (code, page) = d.get("/api/jobs/j-1").await;
    assert_eq!(code, StatusCode::OK, "{page}");
    let ids: Vec<&Value> = page["scope"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| &r["id"])
        .collect();
    assert!(ids.contains(&&json!("c-1")), "{page}");
    assert!(!ids.contains(&&Value::Null), "{page}");
    // The ID of another form does not name it.
    let (code, _) = d
        .post("/api/jobs/j-1/scope/remove", json!({"ref": "r-1"}))
        .await;
    assert_eq!(code, StatusCode::NOT_FOUND);
    let (code, page) = d
        .post("/api/jobs/j-1/scope/remove", json!({"ref": "c-1"}))
        .await;
    assert_eq!(code, StatusCode::OK, "{page}");
    assert!(!d.scope("j-1").contains(&Ref::Item(1)));
    // A done job: a remove is 409, and the scope stays.
    let scope = d.scope("j-1");
    assert!(!scope.is_empty());
    iris_job::finish(
        &d.1,
        1,
        iris_job::By::Person,
        iris_job::End::Cancel,
        None,
        NOW,
    )
    .unwrap();
    for r in &scope {
        let id = match r {
            Ref::Thread(t) => format!("t-{t}"),
            Ref::Item(i) => format!("c-{i}"),
        };
        let (code, v) = d
            .post("/api/jobs/j-1/scope/remove", json!({"ref": id}))
            .await;
        assert_eq!(code, StatusCode::CONFLICT, "{id}: {v}");
    }
    assert_eq!(d.scope("j-1"), scope);
    // The store refuses a revoke of a done job too.
    let r = iris_job::revoke_scope(&d.1, 1, scope[0], iris_job::By::Person, NOW + 1);
    assert!(matches!(r, Err(iris_job::Error::Refused(_))), "{r:?}");
    let raw = d.1.write(|tx| {
        Ok(tx.execute(
            "UPDATE job_scope SET revoked_by = 'person', revoked_at = 1 WHERE job_id = 1 AND revoked_at IS NULL",
            [],
        )?)
    });
    assert!(raw.is_err(), "{raw:?}");
}

/// Decision D26 (issue #558): a job that starts from an item can wait for a
/// reply on the thread of the item, and a reply on that thread wakes it.
#[tokio::test]
async fn a_job_from_an_item_waits_for_a_reply_on_the_thread_of_the_item() {
    let d = open("reply");
    let (code, job) = d.start("c-1").await;
    assert_eq!(code, StatusCode::CREATED, "{job}");
    let job = iris_job::parse_job_id(job["id"].as_str().unwrap()).unwrap();
    let session = iris_job::register(&d.1, job, "claude-code", NOW).unwrap();
    let wait = |thread| iris_job::Wait {
        thread: Some(thread),
        until_ms: NOW + 86_400_000,
    };
    // Thread 3 holds no source of the item: not in the scope.
    let r = iris_job::session_wait(&d.1, session, job, wait(3), None, NOW + 1);
    assert!(matches!(r, Err(iris_job::Error::Refused(_))), "{r:?}");
    // Thread 2 holds the update of commitment 1.
    iris_job::session_wait(&d.1, session, job, wait(2), None, NOW + 1).unwrap();
    assert!(iris_workspace::wake_due(&d.1, NOW + 2).unwrap().is_empty());
    d.1.write(|tx| {
        tx.execute_batch(
            "INSERT INTO message (id, account_id, thread_id, provider_id, internal_date, understood_at)
               VALUES (20, 1, 2, 'm20', 9000, 1);
             INSERT INTO header (message_id, date, from_addr, subject)
               VALUES (20, 9000, 'dana@example.com', 'Re: Section 4');",
        )?;
        Ok(())
    })
    .unwrap();
    assert_eq!(iris_workspace::wake_due(&d.1, NOW + 3).unwrap(), vec![job]);
    let last = iris_job::events(&d.1, job).unwrap().pop().unwrap();
    assert_eq!(last.why.as_deref(), Some("reply"));
}

/// Decision D3: a grant of `draft` alone allows drafts of the agent to the
/// people in the thread.
#[tokio::test]
async fn a_draft_grant_allows_drafts_to_the_thread() {
    let d = open("draftgrant");
    d.start("c-1").await;
    let (code, p) = d
        .post(
            "/api/policy/grant",
            json!({"job": "j-1", "verbs": ["draft"], "version": null}),
        )
        .await;
    assert_eq!(code, StatusCode::OK, "{p}");
    let rule = &p["rules"][0];
    assert_eq!(rule["verbs"], json!(["draft"]));
    assert_eq!(rule["when"]["recipients"], json!("thread"));
    assert_eq!(rule["when"]["author"], json!("agent"));
}
