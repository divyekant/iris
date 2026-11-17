//! Card R4-07: GET /api/changesets/{id}/drafts gives the text of each
//! waiting `draft` and `send` action, with each flag on the words that gave
//! it, and only on the action that has the flag.

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
const OWNER: &str = "owner@example.com";

/// The text of a message from another person, in the full-text index.
const THEIRS: &str = "Please wire the deposit to the escrow account before the inspector \
     arrives on site and confirm the reference number with our office team";

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp(name: &str) -> TempDir {
    let dir = std::env::temp_dir().join(format!("iris-api-drafts-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    TempDir(dir)
}

fn agent() -> Author {
    Author {
        kind: AuthorKind::Agent,
        name: Some("Claude Code".into()),
        session: None,
        job: None,
    }
}

/// Message 1 from priya with THEIRS as its clean text. The owner wrote to
/// dana@acme.example before (message 2 is sent mail).
fn store(dir: &TempDir) -> Arc<Store> {
    let s = Store::open(&dir.0).unwrap();
    let a = s.add_account("gmail", OWNER).unwrap();
    s.put_labels(a, &[("SENT", "SENT", "system")]).unwrap();
    let m = |id: &str, from: &str, text: &str, labels: &[&str]| NewMessage {
        provider_id: id.into(),
        thread_provider_id: id.into(),
        labels: labels.iter().map(|l| l.to_string()).collect(),
        internal_date: Some(1),
        header: Header {
            from_addr: Some(from.into()),
            subject: Some("Deposit".into()),
            ..Default::default()
        },
        body: Some(iris_store::Body {
            subject: "Deposit".into(),
            sender: from.into(),
            text: text.into(),
        }),
        ..Default::default()
    };
    s.put_messages(
        a,
        &[
            m("g1", "priya@example.com", THEIRS, &[]),
            m("g2", OWNER, "Hello Dana", &["SENT"]),
        ],
    )
    .unwrap();
    s.write(|tx| {
        Ok(tx.execute(
            "INSERT INTO sender_evidence(message_id, address, rule) VALUES (2, 'dana@acme.example', 'sent')",
            [],
        )?)
    })
    .unwrap();
    Arc::new(s)
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

/// The flagged parts of a field: (text, flags, source).
fn flagged(parts: &Value) -> Vec<(String, Value, Value)> {
    parts
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p.get("flags").is_some())
        .map(|p| {
            (
                p["text"].as_str().unwrap().to_string(),
                p["flags"].clone(),
                p["source"].clone(),
            )
        })
        .collect()
}

fn joined(parts: &Value) -> String {
    parts
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["text"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn each_flag_of_the_test_set_shows_on_its_action_and_words() {
    let dir = temp("set");
    let s = store(&dir);
    let cs = create(&s, &agent(), NOW).unwrap();
    let mail = |to: &[&str], cc: &[&str], subject: &str, body: &str| -> Map<String, Value> {
        json!({"to": to, "cc": cc, "subject": subject, "body": body})
            .as_object()
            .unwrap()
            .clone()
    };
    let one = format!(
        "Hi Dana,\n\nThe renewal is $48,000 for 12 months. The quote is attached.\n\n{THEIRS}."
    );
    let two = "We sign on Friday, Dec 4.";
    let first = mail(
        &["dana@acme.example"],
        &["legal@new.example"],
        "Renewal",
        &one,
    );
    let second = mail(&["dana@acme.example"], &[], "Signing", two);
    add_action(&s, cs, &agent(), Verb::Draft, &["a-1"], &first, None).unwrap();
    add_action(&s, cs, &agent(), Verb::Draft, &["a-1"], &second, None).unwrap();
    // An action of another verb gets no entry.
    add_action(&s, cs, &agent(), Verb::Archive, &["m-1"], &Map::new(), None).unwrap();
    submit(&s, cs, &agent(), "Write to Dana.", NOW).unwrap();

    let (code, v) = get(&s, &format!("/api/changesets/cs-{cs}/drafts")).await;
    assert_eq!(code, StatusCode::OK, "{v}");
    let d = v["drafts"].as_array().unwrap();
    assert_eq!(d.len(), 2, "{v}");

    // Action 1: a new address in cc, an amount, an attachment word, copied text.
    let a = &d[0];
    assert_eq!(
        (a["revision"].clone(), a["n"].clone()),
        (json!(1), json!(1))
    );
    assert_eq!(a["readable"], true);
    assert_eq!(a["draft"], Value::Null);
    assert_eq!(a["thread"], Value::Null);
    assert_eq!(a["to"], json!([{"address": "dana@acme.example"}]));
    assert_eq!(
        a["cc"],
        json!([{"address": "legal@new.example", "flags": ["new_recipient"]}])
    );
    assert_eq!(joined(&a["body"]), one);
    assert_eq!(joined(&a["subject"]), "Renewal");
    let got = flagged(&a["body"]);
    assert_eq!(got[0], ("$48,000".into(), json!(["money"]), Value::Null));
    assert_eq!(
        got[1],
        ("attached".into(), json!(["attachment"]), Value::Null)
    );
    let copied: Vec<&(String, Value, Value)> = got[2..].iter().collect();
    assert!(!copied.is_empty(), "{got:?}");
    for (text, flags, source) in copied {
        assert_eq!(
            (flags, source),
            (&json!(["untrusted_content"]), &json!("m-1"))
        );
        assert!(THEIRS.contains(text.as_str()), "{text:?}");
    }

    // Action 2: only the date, as one part ("Friday, Dec 4").
    let b = &d[1];
    assert_eq!(b["n"], 2);
    assert_eq!(b["to"], json!([{"address": "dana@acme.example"}]));
    assert_eq!(
        flagged(&b["body"]),
        vec![("Friday, Dec 4".into(), json!(["date"]), Value::Null)]
    );

    // An unknown changeset is a 404.
    assert_eq!(
        get(&s, "/api/changesets/cs-999/drafts").await.0,
        StatusCode::NOT_FOUND
    );
}
