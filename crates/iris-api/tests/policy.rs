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

#[tokio::test]
async fn a_proposal_goes_into_the_file_only_on_accept() {
    let d = open("proposal");
    d.1.write(|tx| {
        tx.execute_batch(
            "INSERT INTO rule_proposal (verb, cond, value, approvals, created_at, updated_at)
             VALUES ('archive', 'sender', 'deals@acme.example', 12, 5, 5),
                    ('read', 'sender_kind', 'service', 10, 6, 6);",
        )?;
        Ok(())
    })
    .unwrap();
    let v = d.get().await;
    let p = &v["proposals"][0];
    assert_eq!((&p["id"], &p["approvals"]), (&json!(1), &json!(12)), "{v}");
    assert_eq!(p["rule"]["name"], "Archive mail from deals@acme.example");
    assert_eq!(p["rule"]["by"], "proposal");
    assert_eq!(p["rule"]["when"], json!({ "sender": "deals@acme.example" }));
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
