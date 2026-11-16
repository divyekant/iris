//! Tests of the job endpoints (card R4-09): a page of jobs, and one job with
//! its changesets and the mail of its waits.

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use iris_api::Auth;
use iris_changeset::{add_action, create, submit, Author, AuthorKind, Verb};
use iris_job::{ask, finish, wait, By, End, Wait};
use iris_store::{Header, NewMessage, Store};
use serde_json::{json, Map, Value};
use tower::ServiceExt;

const NOW: i64 = 1_794_787_200_000;
const DAY: i64 = 86_400_000;

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp(name: &str) -> TempDir {
    let dir = std::env::temp_dir().join(format!("iris-api-jobs-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    TempDir(dir)
}

/// Messages 1 and 2 in thread 1, message 3 in thread 2.
fn store(dir: &TempDir) -> Arc<Store> {
    let s = Store::open(&dir.0).unwrap();
    let a = s.add_account("gmail", "owner@example.com").unwrap();
    s.put_labels(a, &[("INBOX", "INBOX", "system")]).unwrap();
    let m = |id: &str, t: &str, subject: &str, at: i64| NewMessage {
        provider_id: id.into(),
        thread_provider_id: t.into(),
        labels: vec!["INBOX".into()],
        internal_date: Some(at),
        header: Header {
            date: Some(at),
            from_addr: Some("dana@example.com".into()),
            from_name: Some("Dana".into()),
            subject: Some(subject.into()),
            ..Default::default()
        },
        ..Default::default()
    };
    s.put_messages(
        a,
        &[
            m("g1", "gt1", "The contract", NOW - 2 * DAY),
            m("g2", "gt1", "Re: The contract", NOW - DAY),
            m("g3", "gt2", "News", NOW - DAY),
        ],
    )
    .unwrap();
    Arc::new(s)
}

fn agent(job: Option<&str>) -> Author {
    Author {
        kind: AuthorKind::Agent,
        name: Some("Claude Code".into()),
        session: Some("se-1".into()),
        job: job.map(Into::into),
    }
}

/// A changeset of `job` with one archive action, submitted when `at` is
/// `Some`. Gives its ID.
fn changeset(s: &Store, job: Option<&str>, at: Option<i64>) -> i64 {
    let a = agent(job);
    let cs = create(s, &a, NOW).unwrap();
    add_action(s, cs, &a, Verb::Archive, &["t-2"], &Map::new(), None).unwrap();
    if let Some(at) = at {
        submit(s, cs, &a, "Old news.", at).unwrap();
    }
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

fn ids(page: &Value) -> Vec<&str> {
    page["jobs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|j| j["id"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn pages_the_jobs_of_a_state_newest_first() {
    let dir = temp("page");
    let s = store(&dir);
    let working = iris_job::create(&s, "Book the dentist.", None, NOW).unwrap();
    let asks = iris_job::create(&s, "Pay the invoice.", Some(NOW + DAY), NOW).unwrap();
    ask(&s, asks, "Which card?", None, NOW + 1).unwrap();
    let waits = iris_job::create(&s, "Get the contract.", None, NOW).unwrap();
    let until = Wait {
        thread: Some(1),
        until_ms: NOW + 2 * DAY,
    };
    wait(&s, waits, until, None, NOW + 1).unwrap();
    let done = iris_job::create(&s, "Cancel the gym.", None, NOW).unwrap();
    finish(&s, done, By::Person, End::Cancel, None, NOW + 1).unwrap();
    let j = iris_job::format_job_id;

    let (code, open) = get(&s, "/api/jobs").await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(ids(&open), [j(waits), j(asks), j(working)]);
    assert_eq!(open["next"], Value::Null);
    // A summary is the document without the events.
    let first = &open["jobs"][1];
    assert_eq!(first["question"], "Which card?");
    assert_eq!(first["state"], "needs_you");
    assert!(first.get("events").is_none());
    assert_eq!(open["jobs"][0]["wait"]["thread"], "t-1");

    assert_eq!(ids(&get(&s, "/api/jobs?state=done").await.1), [j(done)]);
    assert_eq!(
        ids(&get(&s, "/api/jobs?state=needs_you").await.1),
        [j(asks)]
    );

    // Pages of one job: each job once, then no next.
    let (_, p1) = get(&s, "/api/jobs?state=open&limit=2").await;
    assert_eq!(ids(&p1), [j(waits), j(asks)]);
    assert_eq!(p1["next"], j(asks));
    let (_, p2) = get(&s, &format!("/api/jobs?limit=2&after={}", j(asks))).await;
    assert_eq!(ids(&p2), [j(working)]);
    assert_eq!(p2["next"], Value::Null);

    for bad in [
        "/api/jobs?state=stopped",
        "/api/jobs?state=",
        "/api/jobs?limit=0",
        "/api/jobs?limit=201",
        "/api/jobs?after=7",
        "/api/jobs?after=j-0",
        "/api/jobs?after=j-01",
        "/api/jobs?after=",
    ] {
        let (code, v) = get(&s, bad).await;
        assert_eq!(code, StatusCode::BAD_REQUEST, "{bad}");
        assert!(v["error"].is_string(), "{bad}");
    }
}

#[tokio::test]
async fn one_job_with_its_changesets_and_the_mail_of_its_waits() {
    let dir = temp("one");
    let s = store(&dir);
    let job = iris_job::create(&s, "Get the contract.", None, NOW).unwrap();
    let id = iris_job::format_job_id(job);
    let other = iris_job::create(&s, "Another goal.", None, NOW).unwrap();
    let until = |d| Wait {
        thread: Some(1),
        until_ms: NOW + d * DAY,
    };
    wait(&s, job, until(2), None, NOW + 1).unwrap();
    iris_job::wake(&s, job, By::Engine, Some("reply"), NOW + 2).unwrap();
    // A wait on a thread that has no message, and a wait with no thread.
    wait(
        &s,
        job,
        Wait {
            thread: Some(99),
            until_ms: NOW + DAY,
        },
        None,
        NOW + 3,
    )
    .unwrap();
    iris_job::wake(&s, job, By::Person, None, NOW + 4).unwrap();
    wait(
        &s,
        job,
        Wait {
            thread: None,
            until_ms: NOW + DAY,
        },
        None,
        NOW + 5,
    )
    .unwrap();

    let later = changeset(&s, Some(&id), Some(NOW + 20));
    let sooner = changeset(&s, Some(&id), Some(NOW + 10));
    changeset(&s, Some(&id), None); // A draft is not in the timeline.
    changeset(&s, Some(&iris_job::format_job_id(other)), Some(NOW + 10));
    changeset(&s, None, Some(NOW + 10));

    let (code, v) = get(&s, &format!("/api/jobs/{id}")).await;
    assert_eq!(code, StatusCode::OK);
    let mut doc = Vec::new();
    iris_job::write_document(&s, job, &mut doc).unwrap();
    assert_eq!(v["job"], serde_json::from_slice::<Value>(&doc).unwrap());

    let cs: Vec<&str> = v["changesets"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    let f = iris_changeset::format_changeset_id;
    assert_eq!(cs, [f(sooner), f(later)], "the first submit first");
    let c = &v["changesets"][0];
    assert_eq!(c["why"], "Old news.");
    // The policy of the first start denies the action, so the changeset closed.
    assert_eq!(c["state"], "closed");
    assert_eq!(c["at"], iris_api::rfc3339(NOW + 10));
    assert_eq!(c["actions"], json!({ "denied": 1 }));

    let threads = v["threads"].as_object().unwrap();
    assert_eq!(threads.len(), 2, "{threads:?}");
    // The newest message of the thread.
    assert_eq!(threads["t-1"]["id"], "m-2");
    assert_eq!(threads["t-1"]["subject"], "Re: The contract");
    assert_eq!(threads["t-1"]["gone"], false);
    assert_eq!(threads["t-99"], json!({ "id": "t-99", "gone": true }));

    // The other job has its own changeset and no wait.
    let (_, v) = get(&s, &format!("/api/jobs/j-{other}")).await;
    assert_eq!(v["changesets"].as_array().unwrap().len(), 1);
    assert_eq!(v["threads"], json!({}));

    for missing in ["j-99", "j-0", "j-01", "7", "cs-1", "j-1x", "j-%2B1"] {
        assert_eq!(
            get(&s, &format!("/api/jobs/{missing}")).await.0,
            StatusCode::NOT_FOUND,
            "{missing}"
        );
    }
}

/// Issue #303: a job with an event that does not parse gives a 500 with
/// the JSON error, not a partial document.
#[tokio::test]
async fn a_bad_event_gives_an_error_and_no_partial_job() {
    let dir = temp("bad");
    let s = store(&dir);
    let job = iris_job::create(&s, "Get the contract.", None, NOW).unwrap();
    ask(&s, job, "Which copy?", None, NOW + 1).unwrap();
    s.write(|tx| {
        Ok(tx.execute(
            "INSERT INTO job_event (job_id, at, state, by, answer)
               VALUES (?1, ?2, 'working', 'person', CAST(x'80' AS TEXT))",
            [job, NOW + 2],
        )?)
    })
    .unwrap();
    // No byte: also a writer that sends each byte at once gets nothing.
    let mut out = Vec::new();
    assert!(iris_api::write_job(&s, job, &mut out).is_err());
    assert!(out.is_empty(), "{}", String::from_utf8_lossy(&out));
    let (code, v) = get(&s, &format!("/api/jobs/j-{job}")).await;
    assert_eq!(code, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(v["error"].is_string(), "{v}");
}

/// A writer that runs `f` once, at its first byte.
struct OnFirst<F: FnMut()> {
    f: Option<F>,
    out: Vec<u8>,
}

impl<F: FnMut()> std::io::Write for OnFirst<F> {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        if let Some(mut f) = self.f.take() {
            f();
        }
        self.out.extend_from_slice(data);
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The whole answer of one job comes from one read: a write during the
/// answer (a sync marks the newest mail of the wait thread gone) does not
/// show in a later part, so no part disagrees with another.
#[tokio::test]
async fn one_job_is_one_read_of_the_store() {
    let dir = temp("snapshot");
    let s = store(&dir);
    let job = iris_job::create(&s, "Get the contract.", None, NOW).unwrap();
    let until = Wait {
        thread: Some(1),
        until_ms: NOW + DAY,
    };
    wait(&s, job, until, None, NOW + 1).unwrap();
    let id = iris_job::format_job_id(job);
    let s2 = s.clone();
    let mut w = OnFirst {
        f: Some(|| {
            s2.write(|tx| Ok(tx.execute("UPDATE message SET gone_at = 1 WHERE id = 2", [])?))
                .unwrap();
            changeset(&s2, Some(&id), Some(NOW + 10));
        }),
        out: Vec::new(),
    };
    iris_api::write_job(&s, job, &mut w).unwrap();
    let v: Value = serde_json::from_slice(&w.out).unwrap();
    assert_eq!(
        v["changesets"],
        json!([]),
        "the changeset came after the read"
    );
    assert_eq!(v["threads"]["t-1"]["id"], "m-2");
    assert_eq!(v["threads"]["t-1"]["gone"], false);
    // A new read sees the write: the newest message that is not gone.
    let (_, v) = get(&s, &format!("/api/jobs/{}", iris_job::format_job_id(job))).await;
    assert_eq!(v["threads"]["t-1"]["id"], "m-1");
    assert_eq!(v["changesets"].as_array().unwrap().len(), 1);
}
