//! Tests of the policy endpoints (spec 2 section 6.3).

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use iris_api::Auth;
use iris_store::Store;
use serde_json::{json, Value};
use tower::ServiceExt;

struct Dir(PathBuf, Arc<Store>, Arc<Auth>);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn open(name: &str) -> Dir {
    let dir = std::env::temp_dir().join(format!("iris-api-policy-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Arc::new(Store::open(&dir).unwrap());
    let auth = Arc::new(Auth::new("127.0.0.1:4800".parse().unwrap()).unwrap());
    Dir(dir, store, auth)
}

impl Dir {
    async fn call(
        &self,
        method: &str,
        uri: &str,
        body: Option<Value>,
        signed_in: bool,
    ) -> (StatusCode, Value) {
        let mut req = Request::builder()
            .method(method)
            .uri(uri)
            .header("host", "127.0.0.1:4800");
        if signed_in {
            req = req.header("authorization", self.2.bearer());
        }
        let body = match body {
            Some(v) => {
                req = req.header("content-type", "application/json");
                Body::from(v.to_string())
            }
            None => Body::empty(),
        };
        let res = iris_api::router(self.1.clone(), self.2.clone())
            .oneshot(req.body(body).unwrap())
            .await
            .unwrap();
        let code = res.status();
        let bytes = to_bytes(res.into_body(), 1 << 20).await.unwrap();
        (code, serde_json::from_slice(&bytes).unwrap())
    }

    async fn get(&self) -> Value {
        let (code, v) = self.call("GET", "/api/policy", None, true).await;
        assert_eq!(code, StatusCode::OK, "{v}");
        v
    }

    async fn post(&self, what: &str, body: Value) -> (StatusCode, Value) {
        self.call("POST", &format!("/api/policy/{what}"), Some(body), true)
            .await
    }

    fn write(&self, text: &str) {
        std::fs::write(self.0.join(iris_policy::FILE), text).unwrap();
    }
}

const FILE: &str = r#"version = 0
mode = "apply"

[[rule]]
id = 1
name = "Hide Acme Deals"
effect = "hide"
when = { sender = "deals@acme.example" }
by = "person"
added = 2026-11-12

[[rule]]
id = 2
name = "Archive newsletters after 3 days"
effect = "allow"
verbs = ["archive", "read"]
when = { form = "feed", sender_kind = "service", older_than_days = 3 }
by = "person"
added = 2026-11-10
"#;

#[tokio::test]
async fn the_first_start_has_no_rules_and_no_file() {
    let d = open("first");
    let v = d.get().await;
    assert_eq!(
        v,
        json!({ "mode": "observe", "version": null, "next_id": 1, "rules": [], "proposals": [], "error": null, "file": { "state": "missing" } })
    );
    let (code, status) = d.call("GET", "/api/status", None, true).await;
    assert_eq!((code, &status["mode"]), (StatusCode::OK, &json!("observe")));
}

#[tokio::test]
async fn a_changed_file_shows_its_difference_and_needs_the_hash_that_the_person_saw() {
    let d = open("confirm");
    d.write(FILE);
    let v = d.get().await;
    let file = &v["file"];
    assert_eq!(file["state"], "changed");
    assert_eq!(file["mode"], "apply");
    assert_eq!(file["text"], FILE);
    assert_eq!(file["sha256"], iris_policy::sha256_hex(FILE));
    assert_eq!(file["added"].as_array().unwrap().len(), 2);
    assert_eq!(
        file["added"][1],
        json!({
            "id": 2, "name": "Archive newsletters after 3 days", "effect": "allow",
            "verbs": ["archive", "read"],
            "when": { "form": "feed", "sender_kind": "service", "older_than_days": 3 },
            "by": "person", "added": "2026-11-10", "job": null, "line": 12,
        })
    );
    // Nothing is active before the confirmation.
    assert_eq!(
        (v["mode"].clone(), v["rules"].clone()),
        (json!("observe"), json!([]))
    );

    let (code, e) = d
        .post(
            "confirm",
            json!({ "sha256": iris_policy::sha256_hex("other") }),
        )
        .await;
    assert_eq!(code, StatusCode::CONFLICT, "{e}");
    assert_eq!(d.get().await["version"], json!(null));

    let (code, v) = d.post("confirm", json!({ "sha256": file["sha256"] })).await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(
        (v["version"].clone(), v["mode"].clone()),
        (json!(1), json!("apply"))
    );
    assert_eq!(v["rules"].as_array().unwrap().len(), 2);
    assert_eq!(v["file"], json!({ "state": "same" }));
    let (_, status) = d.call("GET", "/api/status", None, true).await;
    assert_eq!(status["mode"], "apply");

    // A second edit shows only what changed.
    d.write(
        &FILE
            .replace("Hide Acme Deals", "Hide Acme")
            .replace("mode = \"apply\"", "mode = \"observe\""),
    );
    let file = d.get().await["file"].clone();
    assert_eq!(file["mode"], "observe");
    assert_eq!(
        (file["added"].clone(), file["removed"].clone()),
        (json!([]), json!([]))
    );
    assert_eq!(file["changed"][0]["before"]["name"], "Hide Acme Deals");
    assert_eq!(file["changed"][0]["after"]["name"], "Hide Acme");
}

#[tokio::test]
async fn a_change_writes_the_file_and_needs_the_version_that_the_person_saw() {
    let d = open("change");
    d.write(FILE);
    d.post(
        "confirm",
        json!({ "sha256": iris_policy::sha256_hex(FILE) }),
    )
    .await;

    let (code, v) = d.post("remove", json!({ "version": 1, "id": 1 })).await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(v["version"], 2);
    assert_eq!(
        v["rules"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].clone())
            .collect::<Vec<_>>(),
        [json!(2)]
    );
    assert_eq!(v["file"], json!({ "state": "same" }));
    // A new rule takes an ID that no rule had.
    assert_eq!(v["next_id"], 3);

    let disk = std::fs::read_to_string(d.0.join(iris_policy::FILE)).unwrap();
    assert!(
        !disk.contains("Acme") && disk.contains("Archive newsletters"),
        "{disk}"
    );

    // The person saw version 1; version 2 is active now.
    let (code, _) = d
        .post("mode", json!({ "version": 1, "mode": "observe" }))
        .await;
    assert_eq!(code, StatusCode::CONFLICT);
    let (code, v) = d
        .post("mode", json!({ "version": 2, "mode": "observe" }))
        .await;
    assert_eq!(
        (code, v["mode"].clone()),
        (StatusCode::OK, json!("observe"))
    );

    let (code, _) = d.post("remove", json!({ "version": 3, "id": 1 })).await;
    assert_eq!(code, StatusCode::CONFLICT);
    let (code, e) = d
        .post("mode", json!({ "version": 3, "mode": "auto" }))
        .await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "{e}");
    let (code, e) = d.post("remove", json!({ "id": "one" })).await;
    assert_eq!(code, StatusCode::BAD_REQUEST, "{e}");
    assert!(e["error"].is_string());

    // With the last rule gone, a new rule still takes an ID that no rule had.
    let (_, v) = d.post("remove", json!({ "version": 3, "id": 2 })).await;
    assert_eq!(
        (v["rules"].clone(), v["next_id"].clone()),
        (json!([]), json!(3))
    );
}

#[tokio::test]
async fn a_file_that_is_not_valid_shows_its_line_and_can_be_discarded() {
    let d = open("invalid");
    d.write("version = 0\nmode = \"auto\"\n");
    let v = d.get().await;
    assert_eq!(v["file"]["state"], "invalid");
    assert_eq!(v["file"]["error"]["line"], 2);
    assert!(v["file"]["error"]["message"]
        .as_str()
        .unwrap()
        .contains("auto"));
    let (code, _) = d
        .post("mode", json!({ "version": null, "mode": "apply" }))
        .await;
    assert_eq!(code, StatusCode::CONFLICT);

    let (code, v) = d.post("discard", json!({})).await;
    assert_eq!(
        (code, v["file"].clone()),
        (StatusCode::OK, json!({ "state": "missing" }))
    );
    let saved = v["saved"].as_str().unwrap();
    assert!(saved.starts_with("policy.toml.saved-"), "{saved}");
    assert_eq!(
        std::fs::read_to_string(d.0.join(saved)).unwrap(),
        "version = 0\nmode = \"auto\"\n"
    );
}

/// Each change names the version that the person saw: `null` is the policy
/// of the first start, and a body with no `version` field is refused (#145).
#[tokio::test]
async fn a_change_with_no_version_field_is_refused() {
    let d = open("no-version");
    for (what, body) in [
        ("mode", json!({ "mode": "apply" })),
        ("remove", json!({ "id": 1 })),
        ("accept", json!({ "id": 1 })),
        ("grant", json!({ "job": "j-1", "verbs": ["archive"] })),
    ] {
        let (code, v) = d.post(what, body).await;
        assert_eq!(code, StatusCode::BAD_REQUEST, "{what}: {v}");
    }
    assert_eq!(d.get().await["version"], json!(null));
    assert!(!d.0.join(iris_policy::FILE).exists());

    let (code, v) = d
        .post("mode", json!({ "version": null, "mode": "apply" }))
        .await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(v["version"], 1);
}

#[tokio::test]
async fn each_policy_endpoint_needs_the_sign_in() {
    let d = open("auth");
    for (method, uri) in [
        ("GET", "/api/policy"),
        ("POST", "/api/policy/confirm"),
        ("POST", "/api/policy/discard"),
        ("POST", "/api/policy/mode"),
        ("POST", "/api/policy/remove"),
        ("POST", "/api/policy/accept"),
        ("POST", "/api/policy/dismiss"),
    ] {
        let (code, _) = d.call(method, uri, Some(json!({})), false).await;
        assert_eq!(code, StatusCode::UNAUTHORIZED, "{uri}");
    }
    assert!(!d.0.join(iris_policy::FILE).exists());
}

/// `n` archive actions of an agent on one new message from `from` each,
/// that the person decided with `state`. Then a pass of the engine, if `pass`.
fn decided(store: &Store, from: &str, n: usize, state: &str, pass: bool) {
    store
        .write(|tx| {
            tx.execute_batch(
                "INSERT OR IGNORE INTO account(id, provider, address) VALUES (1, 'gmail', 'me@x.example');
                 INSERT OR IGNORE INTO thread(id, account_id, provider_id) VALUES (1, 1, 't');
                 INSERT OR IGNORE INTO sender(address, kind) VALUES ('deals@acme.example', 'service');",
            )?;
            for _ in 0..n {
                let k: i64 = tx.query_row("SELECT count(*) FROM message", [], |r| r.get(0))?;
                tx.execute(
                    "INSERT INTO message(account_id, thread_id, provider_id, internal_date) VALUES (1, 1, ?1, 0)",
                    [format!("m{k}")],
                )?;
                let m = tx.last_insert_rowid();
                tx.execute("INSERT INTO header(message_id, from_addr) VALUES (?1, ?2)", rusqlite::params![m, from])?;
                tx.execute("INSERT INTO changeset(author_kind, created_at) VALUES ('agent', 0)", [])?;
                let cs = tx.last_insert_rowid();
                tx.execute(
                    "INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args, hash)
                     VALUES (?1, 1, 1, 'archive', ?2, '{}', 'h')",
                    rusqlite::params![cs, format!("{{\"messages\":[\"m-{m}\"]}}")],
                )?;
                let a = tx.last_insert_rowid();
                tx.execute("INSERT INTO action_message(action_id, message_id) VALUES (?1, ?2)", [a, m])?;
                tx.execute("UPDATE changeset SET why = 'w', submitted_at = 1 WHERE id = ?1", [cs])?;
                tx.execute(
                    "UPDATE changeset_action SET state = ?2, decided_by = 'person', decided_at = 2,
                     reason = CASE ?2 WHEN 'denied' THEN 'no' END WHERE id = ?1",
                    rusqlite::params![a, state],
                )?;
            }
            Ok(())
        })
        .unwrap();
    if pass {
        iris_policy::propose::refresh(store, 1_800_000_000, &mut None).unwrap();
    }
}

#[tokio::test]
async fn a_proposal_goes_into_the_file_only_on_accept() {
    let d = open("proposal");
    decided(&d.1, "deals@acme.example", 12, "approved", true);
    let v = d.get().await;
    let p = &v["proposals"][0];
    assert_eq!((&p["id"], &p["approvals"]), (&json!(1), &json!(12)), "{v}");
    assert_eq!(p["kind"], json!("approvals"), "{v}");
    assert_eq!(p["rule"]["name"], "Archive mail from deals@acme.example");
    assert_eq!(p["rule"]["by"], "proposal");
    assert_eq!(p["rule"]["when"], json!({ "sender": "deals@acme.example" }));
    assert_eq!(
        v["proposals"][1]["rule"]["when"],
        json!({ "sender_kind": "service" })
    );
    assert!(
        !d.0.join(iris_policy::FILE).exists(),
        "no file before the accept"
    );

    let (code, v) = d.post("dismiss", json!({ "id": 2 })).await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(v["proposals"].as_array().unwrap().len(), 1);
    let (code, _) = d.post("dismiss", json!({ "id": 2 })).await;
    assert_eq!(code, StatusCode::CONFLICT);

    let (code, v) = d.post("accept", json!({ "version": null, "id": 1 })).await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(v["proposals"], json!([]));
    assert_eq!(v["rules"][0]["by"], "proposal");
    assert_eq!(v["rules"][0]["verbs"], json!(["archive"]));
    let text = std::fs::read_to_string(d.0.join(iris_policy::FILE)).unwrap();
    assert!(text.contains("approved 12 of 12 archive actions"), "{text}");
    let (code, _) = d
        .post("accept", json!({ "version": v["version"], "id": 1 }))
        .await;
    assert_eq!(code, StatusCode::CONFLICT);
}

#[tokio::test]
async fn an_accept_after_a_new_refusal_is_a_409() {
    let d = open("stale");
    decided(&d.1, "deals@acme.example", 10, "approved", true);
    assert_eq!(d.get().await["proposals"][0]["id"], 1);
    // The refusal comes before the next pass of the engine.
    decided(&d.1, "deals@acme.example", 1, "denied", false);
    let (code, e) = d.post("accept", json!({ "version": null, "id": 1 })).await;
    assert_eq!(code, StatusCode::CONFLICT, "{e}");
    assert!(!d.0.join(iris_policy::FILE).exists());
}

/// A `hide` proposal (R5-02) reads as `choices`, with a hide rule for one
/// sender and one form and no verbs.
#[tokio::test]
async fn a_hide_proposal_reads_as_choices() {
    let d = open("hide");
    d.1.write(|tx| {
        Ok(tx.execute(
            "INSERT INTO rule_proposal (verb, cond, value, form, approvals, created_at, updated_at)
             VALUES ('hide', 'sender', 'news@shop.example', 'feed', 11, 5, 5)",
            [],
        )?)
    })
    .unwrap();
    let v = d.get().await;
    let p = &v["proposals"][0];
    assert_eq!(p["kind"], json!("choices"), "{v}");
    assert_eq!(p["approvals"], json!(11));
    assert_eq!(p["rule"]["effect"], json!("hide"));
    assert_eq!(p["rule"]["verbs"], json!([]));
    assert_eq!(
        p["rule"]["when"],
        json!({ "sender": "news@shop.example", "form": "feed" })
    );
}

/// Issue 143: each policy writer waits for the backup lock, which a backup
/// can hold for minutes. A writer that waits must not stop the other
/// requests: the server has one runtime thread. Each route runs alone, so
/// one route on the runtime thread fails the test.
#[tokio::test]
async fn a_policy_writer_that_waits_for_the_lock_stops_no_other_request() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let d = Arc::new(open("lockwait"));
    d.write(FILE);
    let sha = d.get().await["file"]["sha256"].clone();
    // An open job, so that the grant reaches the lock.
    let job = iris_job::create(&d.1, "Book the dentist.", None, 1_800_000_000).unwrap();
    let job = iris_job::format_job_id(job);
    let calls: Vec<(&str, Option<Value>)> = vec![
        ("confirm", Some(json!({ "sha256": sha }))),
        ("discard", None),
        ("mode", Some(json!({ "version": null, "mode": "observe" }))),
        ("remove", Some(json!({ "version": null, "id": 1 }))),
        ("accept", Some(json!({ "version": null, "id": 1 }))),
        (
            "grant",
            Some(json!({ "version": null, "job": job, "verbs": ["archive"] })),
        ),
    ];
    for (what, body) in calls {
        let lock = d.1.backup_lock().unwrap();
        let released = Arc::new(AtomicBool::new(false));
        // The release in 2 s, so that a blocked thread cannot hang the test.
        let r = released.clone();
        let release = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(2));
            r.store(true, Ordering::SeqCst);
            drop(lock);
        });
        let writer = {
            let d = d.clone();
            tokio::spawn(async move {
                d.call("POST", &format!("/api/policy/{what}"), body, true)
                    .await
            })
        };
        // The writer runs first and comes to the lock.
        tokio::task::yield_now().await;
        let (code, _) = d.call("GET", "/api/policy", None, true).await;
        assert_eq!(code, StatusCode::OK);
        assert!(
            !released.load(Ordering::SeqCst),
            "{what}: the read waited for the lock of the writer"
        );
        writer.await.unwrap();
        release.join().unwrap();
    }
}

/// Issue #141: with no policy file, the policy view waits for the backup
/// lock (a writer or a backup can hold it). Each route that gives the view
/// must wait off the one runtime thread, so another request still runs.
#[tokio::test]
async fn a_policy_view_that_waits_for_the_lock_stops_no_other_request() {
    use std::sync::atomic::{AtomicBool, Ordering};
    let d = Arc::new(open("viewwait"));
    decided(&d.1, "deals@acme.example", 12, "approved", true);
    assert!(!d.0.join(iris_policy::FILE).exists());
    let calls: Vec<(&str, &str, Option<Value>)> = vec![
        ("GET", "/api/policy", None),
        ("POST", "/api/policy/dismiss", Some(json!({ "id": 1 }))),
    ];
    for (method, path, body) in calls {
        let lock = d.1.backup_lock().unwrap();
        let released = Arc::new(AtomicBool::new(false));
        // The release in 2 s, so that a blocked thread cannot hang the test.
        let r = released.clone();
        let release = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(2));
            r.store(true, Ordering::SeqCst);
            drop(lock);
        });
        let view = {
            let d = d.clone();
            tokio::spawn(async move { d.call(method, path, body, true).await })
        };
        // The view runs first and comes to the lock.
        tokio::task::yield_now().await;
        let (code, _) = d.call("GET", "/api/notices", None, true).await;
        assert_eq!(code, StatusCode::OK);
        assert!(
            !released.load(Ordering::SeqCst),
            "{path}: another request waited for the view"
        );
        let (code, v) = view.await.unwrap();
        assert_eq!(code, StatusCode::OK, "{path}: {v}");
        release.join().unwrap();
    }
}

/// #816: a writer commits a new policy while the view waits for the backup
/// lock (the file is missing in the rename gap). The view must show the
/// rules and the version of the same active policy that its file state
/// compares to.
#[tokio::test]
async fn the_view_and_its_file_state_read_one_active_policy() {
    let d = open("race");
    let (held, wait) = std::sync::mpsc::channel();
    let writer = {
        let (s, dir) = (d.1.clone(), d.0.clone());
        std::thread::spawn(move || {
            let lock = s.backup_lock().unwrap();
            held.send(()).unwrap();
            // The view reads the old active policy, then waits for the lock.
            std::thread::sleep(std::time::Duration::from_secs(1));
            std::fs::write(dir.join(iris_policy::FILE), FILE).unwrap();
            let l = iris_policy::load(&dir).unwrap().unwrap();
            s.write(|tx| {
                tx.execute(
                    "INSERT INTO policy_version(text, sha256, confirmed_at) VALUES (?1, ?2, 0)",
                    [&l.text, &l.sha256],
                )?;
                Ok(())
            })
            .unwrap();
            drop(lock);
        })
    };
    wait.recv().unwrap();
    let v = d.get().await;
    writer.join().unwrap();
    assert_eq!(v["file"], json!({ "state": "same" }), "{v}");
    assert_eq!(v["version"], json!(1), "{v}");
    assert_eq!(v["mode"], json!("apply"), "{v}");
    assert_eq!(v["rules"].as_array().map(Vec::len), Some(2), "{v}");
}
