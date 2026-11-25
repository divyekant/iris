//! Tests of the local API on a store filled from the synthetic mailbox.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use axum::body::{to_bytes, Body};
use axum::http::{header, Request, StatusCode};
use iris_api::Auth;
use iris_store::{Body as Text, Header, NewMessage, Store};
use iris_synth::{Mailbox, Sender};
use serde_json::{json, Value};
use tower::ServiceExt;

const COUNT: usize = 120;
/// 2026-11-16T00:00:00Z: later than each synthetic message.
const NOW: i64 = 1_794_787_200_000;

struct TempDir(PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn temp(name: &str) -> TempDir {
    let dir = std::env::temp_dir().join(format!("iris-api-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    TempDir(dir)
}

/// The value of one header of a raw synthetic message, unfolded.
fn field(head: &str, name: &str) -> Option<String> {
    head.split("\r\n")
        .find_map(|l| l.strip_prefix(name)?.strip_prefix(": "))
        .map(str::to_string)
}

/// `"Name" <addr>` to (name, addr).
fn split_addr(v: &str) -> (Option<String>, Option<String>) {
    match v.rsplit_once('<') {
        Some((name, addr)) => (
            Some(name.trim().trim_matches('"').to_string()).filter(|n| !n.is_empty()),
            Some(addr.trim_end_matches('>').to_string()),
        ),
        None => (None, Some(v.trim().to_string())),
    }
}

/// A store with the first `COUNT` synthetic messages, in the label INBOX
/// (and SENT for the mail of the owner). Gives the store and the mailbox.
fn filled(dir: &TempDir) -> (Arc<Store>, Mailbox) {
    filled_n(dir, COUNT)
}

/// [`filled`] with the first `count` synthetic messages.
fn filled_n(dir: &TempDir, count: usize) -> (Arc<Store>, Mailbox) {
    let store = Store::open(&dir.0).unwrap();
    let acc = store.add_account("gmail", "owner@example.com").unwrap();
    store
        .put_labels(
            acc,
            &[("INBOX", "INBOX", "system"), ("SENT", "SENT", "system")],
        )
        .unwrap();
    let mailbox = Mailbox::new(11, count);
    let mut batch = Vec::new();
    for i in 0..count {
        let raw = mailbox.raw(i);
        let truth = mailbox.truth(i);
        let text = String::from_utf8_lossy(&raw).replace("\r\n ", " ");
        let (head, body) = text.split_once("\r\n\r\n").unwrap();
        let (from_name, from_addr) = split_addr(&field(head, "From").unwrap());
        let subject = field(head, "Subject");
        let label = if truth.sender == Sender::Owner {
            "SENT"
        } else {
            "INBOX"
        };
        batch.push(NewMessage {
            provider_id: format!("g{i}"),
            thread_provider_id: format!("gt{}", truth.thread),
            content_sha256: Some(store.put_content(&raw).unwrap()),
            size: Some(raw.len() as i64),
            internal_date: Some(truth.date * 1000),
            fetched_at: Some(truth.date * 1000),
            labels: vec![label.into()],
            header: Header {
                date: Some(truth.date * 1000),
                from_addr: from_addr.clone(),
                from_name: from_name.clone(),
                to_addrs: field(head, "To"),
                subject: subject.clone(),
                ..Default::default()
            },
            body: Some(Text {
                subject: subject.unwrap_or_default(),
                sender: from_addr.unwrap_or_default(),
                text: body.to_string(),
            }),
        });
    }
    store.put_messages(acc, &batch).unwrap();
    (Arc::new(store), mailbox)
}

fn exec(store: &Store, sql: &str, args: impl rusqlite::Params) -> i64 {
    store
        .write(|tx| {
            tx.execute(sql, args)?;
            Ok(tx.last_insert_rowid())
        })
        .unwrap()
}

fn sender(store: &Store, address: &str, kind: Option<&str>) -> String {
    let id = exec(
        store,
        "INSERT INTO sender(address, kind) VALUES (?1, ?2)",
        rusqlite::params![address, kind],
    );
    format!("p-{id}")
}

/// The stored document of item row `row`.
fn exec_doc(store: &Store, row: i64) -> String {
    store
        .read(|c| Ok(c.query_row("SELECT doc FROM item WHERE id = ?1", [row], |r| r.get(0))?))
        .unwrap()
}

/// Add an item with `doc` (merged over the common fields), with message
/// row `source` as its origin. Gives the row ID.
fn add_item(store: &Store, form: &str, status: &str, source: i64, created: i64, doc: Value) -> i64 {
    let mut full = json!({
        "model": "iris/1.0",
        "id": "x-0",
        "form": form,
        "status": "wrong",
        "title": format!("{form} {created}"),
        "sources": [{"message": format!("m-{source}"), "thread": "t-1", "role": "origin", "quote": "q"}],
        "created": "2026-11-01T00:00:00Z",
        "updated": "2026-11-01T00:00:00Z",
        "derived": {"by": "rule", "rule": "test"}
    });
    for (k, v) in doc.as_object().unwrap() {
        full[k] = v.clone();
    }
    let id = exec(
        store,
        "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
         VALUES (?1, 1, ?2, ?3, ?4, ?4)",
        rusqlite::params![form, status, full.to_string(), created],
    );
    exec(
        store,
        "INSERT INTO item_source(item_id, message_id, role) VALUES (?1, ?2, 'origin')",
        [id, source],
    );
    id
}

fn party(p: &str) -> Value {
    json!({"kind": "sender", "sender": p})
}

/// One web secret for all tests, so `call` can sign in each request.
fn auth() -> Arc<Auth> {
    static AUTH: OnceLock<Arc<Auth>> = OnceLock::new();
    AUTH.get_or_init(|| Arc::new(Auth::new("127.0.0.1:4800".parse().unwrap()).unwrap()))
        .clone()
}

/// A request with the token of the sign-in.
async fn call(
    store: &Arc<Store>,
    uri: &str,
    headers: &[(&str, &str)],
) -> (StatusCode, Vec<u8>, axum::http::HeaderMap) {
    call_as(store, uri, headers, Some(&auth().bearer())).await
}

async fn call_as(
    store: &Arc<Store>,
    uri: &str,
    headers: &[(&str, &str)],
    bearer: Option<&str>,
) -> (StatusCode, Vec<u8>, axum::http::HeaderMap) {
    let mut req = Request::get(uri);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    if let Some(b) = bearer {
        req = req.header("authorization", b);
    }
    let res = iris_api::router(store.clone(), auth())
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let code = res.status();
    let h = res.headers().clone();
    let body = to_bytes(res.into_body(), 64 << 20).await.unwrap().to_vec();
    (code, body, h)
}

async fn get(store: &Arc<Store>, uri: &str) -> (StatusCode, Value) {
    let (code, body, _) = call(store, uri, &[("host", "127.0.0.1:4800")]).await;
    (code, serde_json::from_slice(&body).unwrap())
}

fn ids(list: &Value) -> Vec<&str> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|d| d["id"].as_str().unwrap())
        .collect()
}

#[tokio::test]
async fn refuses_a_host_or_origin_that_is_not_the_bound_address() {
    let dir = temp("host");
    let store = Arc::new(Store::open(&dir.0).unwrap());
    // The router is bound to 127.0.0.1:4800 (see `auth`).
    let cases: [(&[(&str, &str)], StatusCode); 10] = [
        (&[("host", "localhost:4800")], StatusCode::OK),
        (
            &[
                ("host", "127.0.0.1:4800"),
                ("origin", "http://127.0.0.1:4800"),
            ],
            StatusCode::OK,
        ),
        // Another port or another loopback name is not the bound address.
        (&[("host", "127.0.0.1:4801")], StatusCode::FORBIDDEN),
        (
            &[
                ("host", "localhost:5173"),
                ("origin", "http://localhost:5173"),
            ],
            StatusCode::FORBIDDEN,
        ),
        (&[("host", "[::1]:4800")], StatusCode::FORBIDDEN),
        // Another port or scheme in Origin is another origin.
        (
            &[
                ("host", "localhost:4800"),
                ("origin", "http://localhost:5173"),
            ],
            StatusCode::FORBIDDEN,
        ),
        (
            &[
                ("host", "localhost:4800"),
                ("origin", "https://localhost:4800"),
            ],
            StatusCode::FORBIDDEN,
        ),
        (&[("host", "evil.example")], StatusCode::FORBIDDEN),
        (
            &[
                ("host", "127.0.0.1:4800"),
                ("origin", "http://evil.example"),
            ],
            StatusCode::FORBIDDEN,
        ),
        (
            &[("host", "127.0.0.1:4800"), ("origin", "null")],
            StatusCode::FORBIDDEN,
        ),
    ];
    for (headers, want) in cases {
        let (code, _, h) = call(&store, "/api/status", headers).await;
        assert_eq!(code, want, "{headers:?}");
        // Each answer has the headers, also a refusal.
        assert_eq!(h["x-content-type-options"], "nosniff");
        assert_eq!(h["cache-control"], "no-store");
    }
    // No Host header at all.
    let (code, _, _) = call(&store, "/api/status", &[]).await;
    assert_eq!(code, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn status_with_no_account_and_with_a_sync() {
    let dir = temp("status");
    let store = Arc::new(Store::open(&dir.0).unwrap());
    let (code, v) = get(&store, "/api/status").await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(v["accounts"], json!([]));
    assert_eq!(v["mode"], "observe");
    assert_eq!(v["messages"]["total"], 0);
    // No model is set: the client says how to set one (issue 740).
    assert_eq!(v["model"], false);
    exec(
        &store,
        "INSERT INTO model_setting(id, provider, base_url, model, version, max_tokens, timeout_s)
         VALUES (1, 'local', 'http://127.0.0.1:1/v1', 'm', 1, 100, 5)",
        [],
    );
    let (_, v) = get(&store, "/api/status").await;
    assert_eq!(v["model"], true);

    let acc = store.add_account("gmail", "owner@example.com").unwrap();
    exec(
        &store,
        "UPDATE account SET sync_kind = 'first', sync_step = 2 WHERE id = ?1",
        [acc],
    );
    exec(
        &store,
        "INSERT INTO sync_log(account_id, kind, started_at) VALUES (?1, 'first', 0)",
        [acc],
    );
    // The sync runs: it holds the run lock of its row (issue #710).
    let row: i64 = store
        .read(|c| Ok(c.query_row("SELECT max(id) FROM sync_log", [], |r| r.get(0))?))
        .unwrap();
    let _sync = store.sync_run_lock(row).unwrap().unwrap();
    let (_, v) = get(&store, "/api/status").await;
    let a = &v["accounts"][0];
    assert_eq!(a["address"], "owner@example.com");
    assert_eq!(a["mirror_ready_at"], Value::Null);
    assert_eq!(a["sync"], json!({"kind": "first", "step": 2}));
    assert_eq!(a["last_sync"]["started"], "1970-01-01T00:00:00Z");
    assert_eq!(a["last_sync"]["ended"], Value::Null);

    // The failure of one message is not a sync: the status keeps the sync row.
    exec(
        &store,
        "INSERT INTO sync_log(account_id, kind, started_at, ended_at, result)
         VALUES (?1, 'fetch', 5, 5, 'failed 18a: HTTP 500')",
        [acc],
    );
    let (_, v) = get(&store, "/api/status").await;
    assert_eq!(v["accounts"][0]["last_sync"]["kind"], "first");
}

#[tokio::test]
async fn search_message_and_raw_source() {
    let dir = temp("search");
    let (store, mailbox) = filled(&dir);

    let (_, v) = get(&store, "/api/status").await;
    assert_eq!(v["messages"]["total"], COUNT as i64);
    assert_eq!(v["messages"]["fetched"], COUNT as i64);

    // A word of the subject of the newest message finds it first.
    let (_, m) = get(&store, &format!("/api/messages/m-{COUNT}")).await;
    let subject = m["subject"].as_str().unwrap().to_string();
    let word = subject
        .split(|c: char| !c.is_alphanumeric())
        .find(|w| w.len() > 3)
        .unwrap();
    let (code, v) = get(&store, &format!("/api/search?q={word}&limit=5")).await;
    assert_eq!(code, StatusCode::OK);
    let hits = v["messages"].as_array().unwrap();
    assert!(!hits.is_empty() && hits.len() <= 5);
    assert_eq!(hits[0]["id"], format!("m-{COUNT}"));
    assert_eq!(hits[0]["subject"], subject.as_str());

    // A search text that is not valid is a 400, with the reason.
    let (code, v) = get(&store, "/api/search?q=from:").await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    assert!(v["error"].as_str().unwrap().contains("from:"));

    // One message, with its labels, its text, its items and its marks.
    let item = add_item(
        &store,
        "feed",
        "new",
        3,
        1,
        json!({"publisher": party("p-1")}),
    );
    add_item(
        &store,
        "feed",
        "gone",
        3,
        2,
        json!({"publisher": party("p-1")}),
    );
    let (_, m) = get(&store, "/api/messages/m-3").await;
    assert_eq!(m["items"], json!([format!("f-{item}")]));
    exec(
        &store,
        "UPDATE message SET hostile = '[\"spam\"]' WHERE id = 3",
        [],
    );
    let (code, m) = get(&store, "/api/messages/m-3").await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(m["id"], "m-3");
    assert!(m["thread"].as_str().unwrap().starts_with("t-"));
    assert!(m["date"].as_str().unwrap().ends_with('Z'));
    assert!(m["from"]["address"].as_str().unwrap().contains('@'));
    assert_eq!(m["labels"].as_array().unwrap().len(), 1);
    assert!(!m["text"].as_str().unwrap().is_empty());
    assert_eq!(m["gone"], false);
    assert_eq!(m["hostile"], json!(["spam"]));
    // No item of hostile mail is in a read (issue 112).
    assert_eq!(m["items"], json!([]));

    // The raw source is the same bytes, as a download.
    let index = (0..COUNT)
        .find(|&i| {
            mailbox.truth(i).date * 1000 == {
                store
                    .read(|c| {
                        Ok(c.query_row(
                            "SELECT internal_date FROM message WHERE id = 3",
                            [],
                            |r| r.get::<_, i64>(0),
                        )?)
                    })
                    .unwrap()
            }
        })
        .unwrap();
    let (code, body, h) = call(
        &store,
        "/api/messages/m-3/raw",
        &[("host", "localhost:4800")],
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(body, mailbox.raw(index));
    assert_eq!(h[header::CONTENT_TYPE], "message/rfc822");
    assert!(h[header::CONTENT_DISPOSITION]
        .to_str()
        .unwrap()
        .starts_with("attachment"));

    // Unknown and bad IDs.
    assert_eq!(
        get(&store, "/api/messages/m-99999").await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(&store, "/api/messages/m-99999/raw").await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(&store, "/api/messages/c-3").await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        get(&store, "/api/messages/x").await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(get(&store, "/api/nothing").await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn items_by_id_and_by_page() {
    let dir = temp("items");
    let (store, _) = filled(&dir);
    let issuer = party("p-1");
    // Made first, from the newest of these messages: the page order is the
    // date of the origin message, not the row order.
    let r1 = add_item(
        &store,
        "record",
        "kept",
        100,
        1,
        json!({"type": "receipt", "issuer": issuer}),
    );
    let r2 = add_item(
        &store,
        "record",
        "kept",
        2,
        2,
        json!({"type": "booking", "issuer": issuer}),
    );
    let r3 = add_item(
        &store,
        "record",
        "corrected",
        3,
        3,
        json!({"type": "receipt", "issuer": issuer}),
    );
    let gone = add_item(
        &store,
        "record",
        "gone",
        4,
        4,
        json!({"type": "receipt", "issuer": issuer}),
    );

    // The row gives the ID and the status, not the stored document.
    let (code, d) = get(&store, &format!("/api/items/r-{r3}")).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(d["id"], format!("r-{r3}"));
    assert_eq!(d["status"], "corrected");
    assert_eq!(
        get(&store, &format!("/api/items/c-{r3}")).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(&store, &format!("/api/items/m-{r3}")).await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        get(&store, &format!("/api/items/r-{gone}")).await.1["status"],
        "gone"
    );

    // Pages, newest first, with no gone item.
    let (_, p) = get(&store, "/api/items?form=record&limit=2").await;
    assert_eq!(ids(&p["items"]), [format!("r-{r1}"), format!("r-{r3}")]);
    assert_eq!(p["next"], format!("r-{r3}"));
    let (_, p) = get(
        &store,
        &format!("/api/items?form=record&limit=2&before=r-{r3}"),
    )
    .await;
    assert_eq!(ids(&p["items"]), [format!("r-{r2}")]);
    assert_eq!(p["next"], Value::Null);

    // Filters.
    let (_, p) = get(&store, "/api/items?form=record&type=receipt").await;
    assert_eq!(ids(&p["items"]), [format!("r-{r1}"), format!("r-{r3}")]);
    let (_, p) = get(&store, "/api/items?status=gone").await;
    assert_eq!(ids(&p["items"]), [format!("r-{gone}")]);
    assert_eq!(
        get(&store, "/api/items?form=folder").await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        get(&store, "/api/items?type=receipt").await.0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        get(&store, "/api/items?before=zz").await.0,
        StatusCode::BAD_REQUEST
    );
    // A status or a type that no form has, or a limit out of range, is a 400.
    for bad in [
        "/api/items?status=opn",
        "/api/items?form=record&status=open",
        "/api/items?form=record&type=reciept",
        "/api/items?limit=0",
        "/api/items?before=m-2",
        "/api/items?form=record&before=c-2",
        "/api/items?limit=201",
        "/api/search?q=a&limit=0",
        // The query string does not parse: still the JSON error.
        "/api/search",
        "/api/items?limit=abc",
    ] {
        let (code, v) = get(&store, bad).await;
        assert_eq!(code, StatusCode::BAD_REQUEST, "{bad}");
        assert!(v["error"].is_string(), "{bad}");
    }
    assert_eq!(
        get(&store, "/api/items?status=open").await.0,
        StatusCode::OK
    );
}

#[tokio::test]
async fn brief_follows_the_queries_of_section_8() {
    let dir = temp("brief");
    let (store, _) = filled(&dir);
    let person = sender(&store, "dana@contoso.example", Some("person"));
    let service = sender(&store, "billing@shop.example", Some("service"));
    let stranger = sender(&store, "who@unknown.example", Some("stranger"));
    let undecided = sender(&store, "new@later.example", None);
    let me = json!({"kind": "self"});
    let owe = |p: &str, due: Value| json!({"direction": "you_owe", "kind": "task", "owner": me, "counterparty": party(p), "due": due});
    let owed = |p: &str| json!({"direction": "owed_to_you", "kind": "reply", "owner": party(p), "counterparty": me});

    // You owe: by due, a commitment with no due last, then by created.
    let late = add_item(
        &store,
        "commitment",
        "open",
        1,
        10,
        owe(&person, json!({"date": "2026-12-01"})),
    );
    let soon = add_item(
        &store,
        "commitment",
        "open",
        2,
        20,
        owe(&service, json!({"at": "2026-11-17T09:00:00Z"})),
    );
    // Later as text than `soon`, but earlier in time (08:00Z).
    let sooner = add_item(
        &store,
        "commitment",
        "open",
        2,
        21,
        owe(&service, json!({"at": "2026-11-17T10:00:00+02:00"})),
    );
    let no_due = add_item(
        &store,
        "commitment",
        "open",
        3,
        5,
        owe(&person, Value::Null),
    );
    add_item(
        &store,
        "commitment",
        "done",
        4,
        1,
        owe(&person, Value::Null),
    );
    add_item(
        &store,
        "commitment",
        "gone",
        4,
        1,
        owe(&person, Value::Null),
    );
    let owe_stranger = add_item(
        &store,
        "commitment",
        "open",
        5,
        30,
        owe(&stranger, Value::Null),
    );
    let owe_undecided = add_item(
        &store,
        "commitment",
        "open",
        5,
        31,
        owe(&undecided, Value::Null),
    );
    // Owed to you: oldest first.
    let new_owed = add_item(&store, "commitment", "open", 6, 50, owed(&person));
    let old_owed = add_item(&store, "commitment", "open", 7, 40, owed(&service));
    let owed_stranger = add_item(&store, "commitment", "open", 7, 41, owed(&stranger));

    let conv = |p: &str, new: i64, turn: bool, at: &str| {
        json!({"participants": [me, party(p)], "role": "direct", "messages": 2, "new": new,
               "your_turn": turn, "latest": {"at": at, "from": party(p), "snippet": "hi"}})
    };
    let c_new = add_item(
        &store,
        "conversation",
        "active",
        8,
        1,
        conv(&person, 1, false, "2026-11-10T00:00:00Z"),
    );
    let c_turn = add_item(
        &store,
        "conversation",
        "active",
        9,
        2,
        conv(&person, 0, true, "2026-11-12T00:00:00Z"),
    );
    add_item(
        &store,
        "conversation",
        "active",
        10,
        3,
        conv(&person, 0, false, "2026-11-13T00:00:00Z"),
    );
    add_item(
        &store,
        "conversation",
        "quiet",
        10,
        4,
        conv(&person, 2, true, "2026-11-13T00:00:00Z"),
    );
    let c_stranger = add_item(
        &store,
        "conversation",
        "active",
        11,
        5,
        conv(&stranger, 1, true, "2026-11-14T00:00:00Z"),
    );

    for (t, s) in [
        ("receipt", "kept"),
        ("receipt", "corrected"),
        ("booking", "kept"),
        ("receipt", "gone"),
    ] {
        add_item(
            &store,
            "record",
            s,
            12,
            1,
            json!({"type": t, "issuer": party(&service)}),
        );
    }
    // A possible duplicate of the booking counts once.
    let original: i64 = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT id FROM item WHERE json_extract(doc, '$.type') = 'booking'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    let dup_booking = add_item(
        &store,
        "record",
        "kept",
        12,
        2,
        json!({"type": "booking", "issuer": party(&service)}),
    );
    exec(
        &store,
        "UPDATE item SET duplicate_of = ?1 WHERE id = ?2",
        [original, dup_booking],
    );
    let f_new = add_item(
        &store,
        "feed",
        "new",
        13,
        1,
        json!({"publisher": party(&service), "published": "2026-11-01T00:00:00Z"}),
    );
    // Two publishers with no address in the documents: the items of one
    // publisher stay together.
    let f_newer = add_item(
        &store,
        "feed",
        "new",
        13,
        3,
        json!({"publisher": party(&service), "published": "2026-11-05T00:00:00Z"}),
    );
    // A publisher with a later sender row, but the first address.
    let first = sender(&store, "aaa@first.example", Some("service"));
    let f_first = add_item(
        &store,
        "feed",
        "new",
        13,
        5,
        json!({"publisher": party(&first), "published": "2026-11-01T00:00:00Z"}),
    );
    let f_other = add_item(
        &store,
        "feed",
        "new",
        13,
        4,
        json!({"publisher": party(&stranger), "published": "2026-11-03T00:00:00Z"}),
    );
    add_item(
        &store,
        "feed",
        "read",
        13,
        2,
        json!({"publisher": party(&service), "published": "2026-11-02T00:00:00Z"}),
    );
    let s_live = add_item(
        &store,
        "signal",
        "active",
        14,
        1,
        json!({"kind": "code", "issuer": party(&service), "expires": "2026-11-16T00:15:00Z"}),
    );
    // Active in the row, but its time passed: the engine did not mark it yet.
    add_item(
        &store,
        "signal",
        "active",
        14,
        2,
        json!({"kind": "code", "issuer": party(&service), "expires": "2026-11-15T23:59:00Z"}),
    );
    // A later text, but an earlier time (2026-11-15T23:00:00Z).
    add_item(
        &store,
        "signal",
        "active",
        14,
        3,
        json!({"kind": "code", "issuer": party(&service), "expires": "2026-11-16T01:00:00+02:00"}),
    );

    // Hostile mail: one marked message and one from a hostile sender, in the
    // last 7 days; one marked message that is older.
    let last_day = NOW - 3_600_000;
    exec(
        &store,
        "UPDATE message SET hostile = '[\"dmarc\"]', internal_date = ?1 WHERE id = 20",
        [last_day],
    );
    exec(
        &store,
        "UPDATE message SET hostile = '[\"spam\"]', internal_date = 0 WHERE id = 21",
        [],
    );
    exec(
        &store,
        "UPDATE message SET hostile = '[]', internal_date = ?1 WHERE id = 22",
        [last_day],
    );
    exec(
        &store,
        "UPDATE message SET internal_date = ?1 WHERE id = 23",
        [last_day],
    );
    exec(
        &store,
        "UPDATE header SET from_addr = 'bad@hostile.example' WHERE message_id = 23",
        [],
    );
    sender(&store, "bad@hostile.example", Some("hostile"));
    // Not understood: 3 failed tries, and not understood.
    exec(
        &store,
        "UPDATE message SET tries = 3 WHERE id IN (30, 31)",
        [],
    );
    exec(
        &store,
        "UPDATE message SET understood_at = 1 WHERE id = 31",
        [],
    );

    // A possible duplicate of `late`: the brief shows the pair once.
    let dup = add_item(
        &store,
        "commitment",
        "open",
        1,
        11,
        owe(&person, json!({"date": "2026-12-01"})),
    );
    exec(
        &store,
        "UPDATE item SET duplicate_of = ?1 WHERE id = ?2",
        [late, dup],
    );
    // A duplicate that is done is not part of the pair in You owe.
    let dup_done = add_item(
        &store,
        "commitment",
        "done",
        1,
        12,
        owe(&person, Value::Null),
    );
    exec(
        &store,
        "UPDATE item SET duplicate_of = ?1 WHERE id = ?2",
        [late, dup_done],
    );

    let b = iris_api::brief(&store, NOW).unwrap();
    let b = serde_json::to_value(b).unwrap();
    let c = |n: i64| format!("c-{n}");
    let v = |n: i64| format!("v-{n}");
    assert_eq!(
        ids(&b["you_owe"]["items"]),
        [c(sooner), c(soon), c(late), c(no_due)]
    );
    assert_eq!(b["you_owe"]["count"], 4);
    assert_eq!(b["you_owe"]["duplicates"], json!({ c(late): [c(dup)] }));
    assert_eq!(ids(&b["owed_to_you"]["items"]), [c(old_owed), c(new_owed)]);
    assert_eq!(ids(&b["conversations"]["items"]), [v(c_turn), v(c_new)]);
    assert_eq!(b["records"], json!({"booking": 1, "receipt": 2}));
    let f = |n: i64| format!("f-{n}");
    assert_eq!(
        ids(&b["feed"]["items"]),
        [f(f_first), f(f_newer), f(f_new), f(f_other)]
    );
    assert_eq!(ids(&b["signals"]["items"]), [format!("s-{s_live}")]);
    assert_eq!(
        ids(&b["strangers"]["items"]),
        [
            c(owed_stranger),
            c(owe_undecided),
            c(owe_stranger),
            v(c_stranger)
        ]
    );
    assert_eq!(b["hostile"], 2);
    assert_eq!(b["not_understood"], 1);
    // Message 21 got the date 0 above: it is the oldest fetched message.
    assert_eq!(b["oldest_fetched"], "1970-01-01T00:00:00Z");
    // The triage proposal: the stored triage changeset, not a count of the
    // brief. Before the first triage run, nothing waits.
    assert_eq!(b["proposal"], json!({"count": 0, "verbs": {}}));
    // Triage leaves out mail that waits for its link check: end the checks,
    // as the first step of a batch run does.
    iris_understand::store::recheck_links(&store, NOW, None).unwrap();
    iris_triage::run(&store, 1, NOW).unwrap();
    let want: u64 = iris_observe::counts(&store, 1)
        .unwrap()
        .iter()
        .map(|(_, n)| n)
        .sum();
    assert!(want > 0);
    let (_, after_run) = get(&store, "/api/brief").await;
    assert_eq!(
        after_run["proposal"],
        json!({"count": want, "verbs": {"archive": want}})
    );
    // Observe mode: no rule decided an action, so Iris handled nothing.
    assert_eq!(after_run["handled"], json!({"count": 0, "verbs": {}}));
    // The old in-memory proposal is gone: the review lists the stored one.
    let (code, _) = get(&store, "/api/proposal").await;
    assert_eq!(code, StatusCode::NOT_FOUND);

    // Pages of one section: the same items in the same order as the brief.
    let mut paged = Vec::new();
    let mut after = String::new();
    loop {
        let (code, p) = get(&store, &format!("/api/brief/you_owe?limit=3{after}")).await;
        assert_eq!(code, StatusCode::OK);
        assert_eq!(p["count"], 4);
        paged.extend(ids(&p["items"]).into_iter().map(str::to_string));
        match p["next"].as_str() {
            Some(n) => after = format!("&after={n}"),
            None => break,
        }
    }
    assert_eq!(paged, ids(&b["you_owe"]["items"]));
    // A page has the duplicates of its own items only.
    let (_, p1) = get(&store, "/api/brief/you_owe?limit=1").await;
    assert_eq!(ids(&p1["items"]), [c(sooner)]);
    assert_eq!(p1["duplicates"], Value::Null);
    let after1 = p1["next"].as_str().unwrap().to_string();
    let (_, p2) = get(
        &store,
        &format!("/api/brief/you_owe?limit=1&after={after1}"),
    )
    .await;
    assert_eq!(ids(&p2["items"]), [c(soon)]);
    let (_, p3) = get(
        &store,
        &format!(
            "/api/brief/you_owe?limit=1&after={}",
            p2["next"].as_str().unwrap()
        ),
    )
    .await;
    assert_eq!(p3["duplicates"], json!({ c(late): [c(dup)] }));
    // The last item of a page leaves the section, and its sort key changes
    // (due later than all others). The next page still starts at `soon`.
    let before = exec_doc(&store, sooner);
    exec(
        &store,
        "UPDATE item SET status = 'done',
           doc = json_set(doc, '$.due', json('{\"date\": \"2027-06-01\"}')) WHERE id = ?1",
        [sooner],
    );
    let (code, p) = get(
        &store,
        &format!("/api/brief/you_owe?limit=1&after={after1}"),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(ids(&p["items"]), [c(soon)]);
    exec(
        &store,
        "UPDATE item SET status = 'open', doc = ?2 WHERE id = ?1",
        rusqlite::params![sooner, before],
    );
    assert_eq!(
        get(&store, "/api/brief/records").await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(&store, "/api/brief/you_owe?limit=0").await.0,
        StatusCode::BAD_REQUEST
    );
    // A cursor is plain keys, not a signed value: keys before every item
    // give the section from the start.
    let made: String = r#"["you_owe",0,null,0,0]"#.bytes().map(|b| format!("{b:02x}")).collect();
    let (code, p) = get(&store, &format!("/api/brief/you_owe?limit=1&after={made}")).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(ids(&p["items"]), [c(sooner)]);
    // An empty `after`, or one that is not a cursor of the section, is a 400.
    let feed_next = get(&store, "/api/brief/feed?limit=1").await.1["next"]
        .as_str()
        .unwrap()
        .to_string();
    for bad in ["", "x", "c-1", "zz", "5b5d", &feed_next] {
        assert_eq!(
            get(&store, &format!("/api/brief/you_owe?after={bad}"))
                .await
                .0,
            StatusCode::BAD_REQUEST,
            "{bad}"
        );
    }

    // The kind of the sender of a message.
    assert_eq!(
        get(&store, "/api/messages/m-23").await.1["sender_kind"],
        "hostile"
    );
    exec(
        &store,
        "UPDATE header SET from_addr = 'DANA@contoso.example' WHERE message_id = 24",
        [],
    );
    assert_eq!(
        get(&store, "/api/messages/m-24").await.1["sender_kind"],
        "person"
    );
    // Rows whose addresses differ only in case: a row with no kind is a
    // stranger and wins over a person, and hostile wins over all.
    sender(&store, "dANA@contoso.example", None);
    assert_eq!(
        get(&store, "/api/messages/m-24").await.1["sender_kind"],
        "stranger"
    );
    // Two sender rows whose addresses differ only in case: hostile wins.
    sender(&store, "Dana@Contoso.example", Some("hostile"));
    assert_eq!(
        get(&store, "/api/messages/m-24").await.1["sender_kind"],
        "hostile"
    );
    exec(
        &store,
        "UPDATE header SET from_addr = 'nobody@nowhere.example' WHERE message_id = 25",
        [],
    );
    assert_eq!(
        get(&store, "/api/messages/m-25").await.1["sender_kind"],
        "stranger"
    );

    // The HTTP route gives the same shape.
    let (code, http) = get(&store, "/api/brief").await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(http["records"], b["records"]);
}

#[tokio::test]
async fn merges_with_the_web_client_router() {
    let dir = temp("merge");
    let store = Arc::new(Store::open(&dir.0).unwrap());
    let app = iris_api::router(store, auth()).merge(iris_web::router());
    let bearer = auth().bearer();
    let send = |uri: &str| {
        let req = Request::get(uri)
            .header("host", "127.0.0.1:4800")
            .header("authorization", &bearer)
            .body(Body::empty())
            .unwrap();
        app.clone().oneshot(req)
    };
    assert_eq!(send("/api/status").await.unwrap().status(), StatusCode::OK);
    // An unknown API path is the 404 of the API, not the web page.
    let res = send("/api/nothing/here").await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert_eq!(res.headers()["content-type"], "application/json");
    // Each other path goes to the web client.
    let res = send("/").await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert!(res.headers()["content-type"]
        .to_str()
        .unwrap()
        .starts_with("text/html"));
}

#[tokio::test]
async fn needs_the_token_of_the_one_time_sign_in() {
    let dir = temp("auth");
    let (store, _) = filled(&dir);
    let auth = Arc::new(Auth::new("127.0.0.1:4800".parse().unwrap()).unwrap());
    let app = iris_api::router(store, auth.clone());
    let send = |method: &str, uri: &str, headers: &[(&str, String)], body: &str| {
        let mut req = Request::builder()
            .method(method)
            .uri(uri)
            .header("host", "127.0.0.1:4800");
        for (k, v) in headers {
            req = req.header(*k, v);
        }
        app.clone()
            .oneshot(req.body(Body::from(body.to_string())).unwrap())
    };
    // Another local user with no token, or a wrong one, reads nothing. A
    // cookie with the secret is not a sign-in: a browser sends cookies to
    // each port of a host.
    let secret = auth.bearer()[7..].to_string();
    for uri in [
        "/api/status",
        "/api/messages/m-1",
        "/api/messages/m-1/raw",
        "/api/nothing",
    ] {
        for headers in [
            vec![],
            vec![("authorization", "Bearer 00".to_string())],
            vec![("cookie", format!("iris_session={secret}"))],
        ] {
            let res = send("GET", uri, &headers, "").await.unwrap();
            assert_eq!(res.status(), StatusCode::UNAUTHORIZED, "{uri} {headers:?}");
        }
    }
    // The sign-in address names the code in the fragment.
    let url = auth.sign_in_url().unwrap();
    let code = url
        .strip_prefix("http://127.0.0.1:4800/#code=")
        .unwrap()
        .to_string();
    // A wrong code, or a GET, does not use the code.
    let res = send("POST", "/api/session", &[], r#"{"code":"nope"}"#)
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    let res = send("GET", &format!("/api/session?code={code}"), &[], "")
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED);
    // The exchange gives the token one time.
    let body = format!(r#"{{"code":"{code}"}}"#);
    let res = send("POST", "/api/session", &[], &body).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let v: Value = serde_json::from_slice(&to_bytes(res.into_body(), 1024).await.unwrap()).unwrap();
    let bearer = format!("Bearer {}", v["token"].as_str().unwrap());
    let res = send(
        "GET",
        "/api/messages/m-1/raw",
        &[("authorization", bearer)],
        "",
    )
    .await
    .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let res = send("POST", "/api/session", &[], &body).await.unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert_eq!(auth.sign_in_url(), None);
}

#[tokio::test]
async fn pages_a_section_larger_than_the_brief() {
    let dir = temp("pages");
    let (store, _) = filled(&dir);
    let a = sender(&store, "a@one.example", Some("service"));
    let b = sender(&store, "b@two.example", Some("service"));
    // 250 feed items: three publishers (one with no sender row and no
    // address, so its sort key is NULL), equal times, and no time.
    for i in 0..250i64 {
        let publisher = [a.as_str(), b.as_str(), "p-99999"][(i % 3) as usize];
        let mut doc = json!({"publisher": party(publisher)});
        if i % 5 != 0 {
            doc["published"] = json!(format!("2026-11-0{}T00:00:00Z", 1 + i % 4));
        }
        add_item(&store, "feed", "new", 13, i, doc);
    }
    let pages = |limit: u32| {
        let store = store.clone();
        async move {
            let mut all = Vec::new();
            let mut after = String::new();
            loop {
                let (code, p) = get(&store, &format!("/api/brief/feed?limit={limit}{after}")).await;
                assert_eq!(code, StatusCode::OK);
                assert_eq!(p["count"], 250);
                all.extend(ids(&p["items"]).into_iter().map(str::to_string));
                match p["next"].as_str() {
                    Some(n) => after = format!("&after={n}"),
                    None => break,
                }
            }
            all
        }
    };
    let big = pages(200).await;
    let small = pages(7).await;
    assert_eq!(big.len(), 250);
    assert_eq!(big, small);
    let unique: std::collections::HashSet<_> = big.iter().collect();
    assert_eq!(unique.len(), 250);
    let (_, brief) = get(&store, "/api/brief").await;
    assert_eq!(ids(&brief["feed"]["items"]), big[..200]);
}

/// The six list sections of the brief.
const LISTS: [&str; 6] = [
    "you_owe",
    "owed_to_you",
    "conversations",
    "feed",
    "signals",
    "strangers",
];

/// The item count of the brief: each list section and each record type.
fn shown_total(b: &Value) -> i64 {
    LISTS
        .iter()
        .map(|s| b[s]["count"].as_i64().unwrap())
        .sum::<i64>()
        + b["records"]
            .as_object()
            .unwrap()
            .values()
            .map(|n| n.as_i64().unwrap())
            .sum::<i64>()
}

#[tokio::test]
async fn each_item_of_the_brief_is_shown_bundled_or_hidden_once() {
    // Invariant (spec 2 section 6.4): each item that the brief can show is
    // in exactly one place: its section (or record count) when shown, the
    // bundle of its rule when bundled, or the hidden count. An item with no
    // `attention` field is shown.
    for seed in 1..=12u64 {
        let dir = temp(&format!("attn-{seed}"));
        let (store, _) = filled(&dir);
        let person = sender(&store, "pat@one.example", Some("person"));
        let service = sender(&store, "news@two.example", Some("service"));
        let stranger = sender(&store, "who@three.example", Some("stranger"));
        let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        let mut rows = Vec::new();
        for i in 0..60i64 {
            let (form, status, doc) = match next() % 9 {
                0 => (
                    "commitment",
                    "open",
                    json!({"direction": "you_owe", "kind": "task", "counterparty": party(&person), "owner": {"kind": "self"}}),
                ),
                1 => (
                    "commitment",
                    "open",
                    json!({"direction": "owed_to_you", "kind": "task", "owner": party(&service), "counterparty": {"kind": "self"}}),
                ),
                2 => (
                    "conversation",
                    "active",
                    json!({"new": 1, "participants": [{"kind": "self"}, party(&person)]}),
                ),
                3 => (
                    "conversation",
                    "active",
                    json!({"new": 1, "participants": [{"kind": "self"}, party(&stranger)]}),
                ),
                4 => ("feed", "new", json!({"publisher": party(&service)})),
                5 => (
                    "signal",
                    "active",
                    json!({"issuer": party(&service), "expires": "2026-11-17T00:00:00Z"}),
                ),
                6 => {
                    let kind = ["receipt", "booking"][(next() % 2) as usize];
                    (
                        "record",
                        "new",
                        json!({"type": kind, "issuer": party(&service)}),
                    )
                }
                // Not in the brief: never counted, whatever its attention.
                7 => (
                    "commitment",
                    "done",
                    json!({"direction": "you_owe", "kind": "task", "counterparty": party(&person)}),
                ),
                _ => (
                    "signal",
                    "active",
                    json!({"issuer": party(&service), "expires": "2026-11-01T00:00:00Z"}),
                ),
            };
            rows.push(add_item(&store, form, status, 13 + i % 40, i, doc));
        }
        // Possible duplicate pairs, one level deep, in the same section or
        // not. The pair counts once (spec 1 section 7.5), whatever the
        // attention of each member.
        let mut originals = Vec::new();
        for (i, row) in rows.iter().enumerate().skip(1) {
            if next() % 3 == 0 {
                let original = rows[(next() % i as u64) as usize];
                if !originals.contains(row) && original != *row {
                    exec(
                        &store,
                        "UPDATE item SET duplicate_of = ?1
                         WHERE id = ?2 AND ?1 NOT IN
                           (SELECT id FROM item WHERE duplicate_of IS NOT NULL)",
                        [original, *row],
                    );
                    originals.push(original);
                }
            }
        }
        let (_, before) = get(&store, "/api/brief").await;
        let universe = shown_total(&before);
        assert_eq!(before["hidden"], 0);
        assert!(before["bundles"].as_array().unwrap().is_empty());
        for row in &rows {
            let attention = match next() % 5 {
                0 => json!({"state": "hidden", "rule": 2, "reason": "r"}),
                1 => json!({"state": "bundled", "rule": 3, "reason": "r"}),
                2 => json!({"state": "bundled", "rule": 7, "reason": "r"}),
                3 => json!({"state": "shown"}),
                _ => continue,
            };
            exec(
                &store,
                "UPDATE item SET doc = json_set(doc, '$.attention', json(?2)) WHERE id = ?1",
                rusqlite::params![row, attention.to_string()],
            );
        }
        let (_, b) = get(&store, "/api/brief").await;
        let bundles = b["bundles"].as_array().unwrap();
        let bundled: i64 = bundles.iter().map(|x| x["count"].as_i64().unwrap()).sum();
        assert_eq!(
            shown_total(&b) + bundled + b["hidden"].as_i64().unwrap(),
            universe,
            "seed {seed}"
        );
        for s in LISTS {
            for item in b[s]["items"].as_array().unwrap() {
                let state = item["attention"]["state"].as_str().unwrap_or("shown");
                assert_eq!(state, "shown", "seed {seed} section {s}");
            }
        }
        let mut rules = Vec::new();
        for bundle in bundles {
            let rule = bundle["rule"].as_u64().unwrap();
            rules.push(rule);
            // No policy names the rule.
            assert_eq!(bundle["name"], format!("Rule {rule}"));
            // The full list of the bundle, page by page.
            let mut all = Vec::new();
            let mut after = String::new();
            loop {
                let (code, p) =
                    get(&store, &format!("/api/brief/bundle-{rule}?limit=7{after}")).await;
                assert_eq!(code, StatusCode::OK);
                for item in p["items"].as_array().unwrap() {
                    assert_eq!(item["attention"]["state"], "bundled");
                    assert_eq!(item["attention"]["rule"], rule);
                    all.push(item["id"].as_str().unwrap().to_string());
                }
                match p["next"].as_str() {
                    Some(n) => after = format!("&after={n}"),
                    None => break,
                }
            }
            assert_eq!(
                all.len() as i64,
                bundle["count"].as_i64().unwrap(),
                "seed {seed}"
            );
            let first: Vec<&str> = ids(&bundle["items"]);
            assert_eq!(
                first,
                all[..first.len()]
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
            );
        }
        let mut sorted = rules.clone();
        sorted.sort();
        assert_eq!(rules, sorted, "bundles in the order of the rule IDs");
        // The list of hidden items, page by page, has each hidden item once,
        // with its rule and reason. With no action, the count of what Iris
        // handled is the hidden count.
        let mut hidden = Vec::new();
        let mut after = String::new();
        loop {
            let (code, p) = get(&store, &format!("/api/brief/hidden?limit=5{after}")).await;
            assert_eq!(code, StatusCode::OK);
            assert_eq!(p["count"], b["hidden"]);
            for item in p["items"].as_array().unwrap() {
                assert_eq!(item["attention"]["state"], "hidden");
                assert_eq!(item["attention"]["reason"], "r");
                hidden.push(item["id"].as_str().unwrap().to_string());
            }
            match p["next"].as_str() {
                Some(n) => after = format!("&after={n}"),
                None => break,
            }
        }
        let mut unique = hidden.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), hidden.len(), "seed {seed}");
        assert_eq!(
            hidden.len() as i64,
            b["hidden"].as_i64().unwrap(),
            "seed {seed}"
        );
        assert_eq!(b["handled"], json!({"count": b["hidden"], "verbs": {}}));
        // No policy: a bundle page is there only while the brief lists it.
        for rule in 1..=9u64 {
            let (code, _) = get(&store, &format!("/api/brief/bundle-{rule}")).await;
            let listed = rules.contains(&rule);
            assert_eq!(code == StatusCode::OK, listed, "seed {seed} rule {rule}");
        }
    }
}

#[tokio::test]
async fn a_bundle_has_the_name_of_its_rule_and_a_bad_bundle_name_is_a_404() {
    let dir = temp("bundle-name");
    let (store, _) = filled(&dir);
    let service = sender(&store, "news@two.example", Some("service"));
    for created in [1, 2] {
        add_item(
            &store,
            "feed",
            "new",
            13,
            created,
            json!({"publisher": party(&service)}),
        );
    }
    let receipt = json!({"type": "receipt", "issuer": party(&service)});
    add_item(&store, "record", "new", 14, 3, receipt);
    std::fs::write(
        store.dir().join(iris_policy::FILE),
        "version = 0\nmode = \"observe\"\n[[rule]]\nid = 3\nname = \"Bundle the feed\"\neffect = \"bundle\"\nwhen = { form = \"feed\" }\nby = \"person\"\nadded = 2026-11-10\n[[rule]]\nid = 4\nname = \"Hide receipts\"\neffect = \"hide\"\nwhen = { record_type = \"receipt\" }\nby = \"person\"\nadded = 2026-11-10\n[[rule]]\nid = 5\nname = \"Bundle bookings\"\neffect = \"bundle\"\nwhen = { record_type = \"booking\" }\nby = \"person\"\nadded = 2026-11-10\n",
    )
    .unwrap();
    let confirm = || {
        let sha = match iris_policy::file_state(&store).unwrap() {
            iris_policy::FileState::Changed(l) => l.sha256,
            other => panic!("{other:?}"),
        };
        // The confirmation writes the attention of each item.
        iris_policy::confirm(&store, &sha, NOW / 1000).unwrap();
    };
    confirm();
    let (_, b) = get(&store, "/api/brief").await;
    assert_eq!(b["bundles"][0]["name"], "Bundle the feed");
    assert_eq!(b["bundles"][0]["count"], 2);
    assert_eq!(b["feed"]["count"], 0);
    // The brief leaves out the hidden receipt. The Records view counts it.
    assert_eq!(b["hidden"], 1);
    assert!(b["records"].get("receipt").is_none());
    let (code, counts) = get(&store, "/api/records").await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(counts, json!({"receipt": 1}));
    // A bundle rule with no item yet has an empty page.
    let (code, p) = get(&store, "/api/brief/bundle-5").await;
    assert_eq!((code, p["count"].clone()), (StatusCode::OK, json!(0)));
    // A rule that is not a bundle rule, or no rule: no bundle.
    for bad in ["bundle-4", "bundle-999", "bundle-0", "bundle-4294967295"] {
        let (code, _) = get(&store, &format!("/api/brief/{bad}")).await;
        assert_eq!(code, StatusCode::NOT_FOUND, "{bad}");
    }
    for bad in ["bundle-03", "bundle-", "bundle-x", "bundle-+3", "bundles-3"] {
        let (code, _) = get(&store, &format!("/api/brief/{bad}")).await;
        assert_eq!(code, StatusCode::NOT_FOUND, "{bad}");
    }
    // A cursor of one bundle is not a cursor of another.
    let (_, p) = get(&store, "/api/brief/bundle-3?limit=1").await;
    let next = p["next"].as_str().unwrap();
    let (code, _) = get(&store, &format!("/api/brief/bundle-4?after={next}")).await;
    assert_eq!(code, StatusCode::BAD_REQUEST);
    let (code, p) = get(&store, &format!("/api/brief/bundle-3?after={next}")).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(p["items"].as_array().unwrap().len(), 1);
    // The owner removes rule 3: its bundle goes, and the feed comes back.
    let text = std::fs::read_to_string(store.dir().join(iris_policy::FILE)).unwrap();
    let start = text.find("[[rule]]\nid = 3").unwrap();
    let end = start + 1 + text[start + 1..].find("[[rule]]").unwrap();
    std::fs::write(
        store.dir().join(iris_policy::FILE),
        format!("{}{}", &text[..start], &text[end..]),
    )
    .unwrap();
    confirm();
    let (code, _) = get(&store, "/api/brief/bundle-3").await;
    assert_eq!(code, StatusCode::NOT_FOUND);
    let (_, b) = get(&store, "/api/brief").await;
    assert_eq!(b["feed"]["count"], 2);
    assert!(b["bundles"].as_array().unwrap().is_empty());
}

/// Gives the answer that the test puts in `next`, once.
struct Oracle(std::cell::RefCell<Option<String>>);

impl iris_understand::Model for Oracle {
    fn name(&self) -> &str {
        "oracle"
    }

    fn ask(&self, _: &str, _: &str) -> Result<iris_model::Reply, iris_model::Error> {
        let text = self.0.borrow_mut().take();
        Ok(iris_model::Reply {
            text: text.unwrap_or_else(|| r#"{"items": [], "phishing": false}"#.into()),
            usage: iris_model::Usage::default(),
            stop: iris_model::Stop::End,
        })
    }
}

/// R3-06: an ask in your own sent mail shows under "Owed to you", with the
/// person you asked and the date. The whole path runs: understanding of the
/// sent message (with a model that gives the true answer), the sender axis,
/// and the brief. The recipient never wrote to you.
#[tokio::test]
async fn an_ask_that_you_send_is_owed_to_you_with_the_person_and_the_date() {
    let dir = temp("owed-sent");
    // Asks of the owner are rare in the synthetic mailbox.
    let (store, mb) = filled_n(&dir, 1500);
    let mut want = Vec::new();
    for i in 0..mb.len() {
        let t = mb.truth(i);
        let f: std::collections::BTreeMap<_, _> = mb.fields(i).into_iter().collect();
        if t.sender != Sender::Owner
            || f.get("direction").map(String::as_str) != Some("owed_to_you")
        {
            continue;
        }
        let raw = String::from_utf8_lossy(&mb.raw(i)).replace("\r\n ", " ");
        let (head, body) = raw.split_once("\r\n\r\n").unwrap();
        let to = split_addr(&field(head, "To").unwrap()).1.unwrap();
        // The subject can be MIME encoded: quote the ask in the body.
        let quote = body.lines().find(|l| l.starts_with("Can you")).unwrap();
        let item = json!({"form": "commitment", "title": "Send it", "quote": quote,
                          "direction": "owed_to_you", "kind": "task",
                          "due": {"date": f["due.date"]}});
        let model = Oracle(std::cell::RefCell::new(Some(
            json!({"items": [item], "phishing": false}).to_string(),
        )));
        let row: i64 = store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT id FROM message WHERE provider_id = ?1",
                    [format!("g{i}")],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        let out = iris_understand::store::understand(&store, row, &model, NOW).unwrap();
        assert_eq!(out.items.len(), 1, "message {i}: {out:?}");
        want.push((to, f["due.date"].clone()));
    }
    assert!(!want.is_empty(), "the mailbox has no sent ask");
    let (code, brief) = get(&store, "/api/brief").await;
    assert_eq!(code, StatusCode::OK);
    let owed = &brief["owed_to_you"];
    assert_eq!(owed["count"], want.len(), "{owed}");
    let mut got: Vec<(String, String)> = owed["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| {
            assert_eq!(d["counterparty"]["kind"], "self", "{d}");
            (
                d["owner"]["address"].as_str().unwrap().to_string(),
                d["due"]["date"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    got.sort();
    want.sort();
    assert_eq!(got, want);
}

#[tokio::test]
async fn the_draft_route_gives_a_ready_nudge_with_its_person() {
    let dir = temp("ready-nudge");
    let (store, _) = filled_n(&dir, 30);
    exec(&store, "UPDATE message SET understood_at = 1 WHERE ?1", [1]);
    exec(&store, "DELETE FROM link_check WHERE ?1", [1]);
    // The newest message of the thread of message 1 is a mail of Dana, not
    // sent by the owner. Dana promised a thing with a due date: a late promise.
    let (thread, newest): (i64, i64) = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT thread_id, (SELECT n.id FROM message n WHERE n.thread_id = m.thread_id
                                     ORDER BY coalesce(n.internal_date, 0) DESC, n.id DESC LIMIT 1)
                   FROM message m WHERE m.id = 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap();
    exec(
        &store,
        "DELETE FROM message_label WHERE message_id = ?1
            AND label_id IN (SELECT id FROM label WHERE provider_id = 'SENT')",
        [newest],
    );
    exec(
        &store,
        "UPDATE header SET from_addr = 'dana@contoso.example' WHERE message_id = ?1",
        [newest],
    );
    let dana = sender(&store, "dana@contoso.example", Some("person"));
    let owed = |p: &str| {
        json!({"direction": "owed_to_you", "kind": "task", "owner": party(p),
               "counterparty": {"kind": "self"}, "due": {"date": "2026-11-01"}})
    };
    let first = add_item(&store, "commitment", "open", 1, 10, owed(&dana));
    // A second commitment of the same thread, owed by Eli: the nudge goes
    // to Dana, so its line is not ready. A third one, owed by Dana again:
    // the one nudge names it too (decision D8).
    let eli = sender(&store, "eli@contoso.example", Some("person"));
    let second = add_item(&store, "commitment", "open", 1, 20, owed(&eli));
    let third = add_item(&store, "commitment", "open", 1, 30, owed(&dana));
    // The nudge that the engine saved: its `ask` is the key of now.
    let key = iris_voice::nudge_key_sql("?1", ("unixepoch('subsec') * 1000", "date('now')"));
    exec(
        &store,
        &format!(
            "INSERT INTO reply_draft(thread_id, based_on, body, model, prompt, samples, created_at, ask)
             VALUES (?1, ?2, 'Any news, Dana?', 'fake', 'nudge/0.1', 0, 1, {key})"
        ),
        [thread, newest],
    );
    let (status, draft) = get(&store, &format!("/api/threads/t-{thread}/draft")).await;
    assert_eq!(status, StatusCode::OK, "{draft}");
    assert_eq!(
        draft,
        json!({"thread": format!("t-{thread}"), "based_on": format!("m-{newest}"),
               "nudge": true, "to": "dana@contoso.example",
               "items": [format!("c-{first}"), format!("c-{third}")],
               "body": "Any news, Dana?"})
    );
    let want = vec![
        (format!("c-{first}"), json!(["draft"])),
        (format!("c-{second}"), Value::Null),
        (format!("c-{third}"), json!(["draft"])),
    ];
    let (_, brief) = get(&store, "/api/brief").await;
    assert_eq!(ready_marks(&brief["owed_to_you"]), want);
    let (_, page) = get(&store, "/api/brief/owed_to_you").await;
    assert_eq!(ready_marks(&page), want);
    // The promise is kept: the nudge is stale.
    exec(
        &store,
        "UPDATE item SET status = 'done' WHERE id = ?1",
        [first],
    );
    let (status, _) = get(&store, &format!("/api/threads/t-{thread}/draft")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// The draft marks of the items of `section` of a brief: the item ID and
/// its `ready` field.
fn ready_marks(section: &Value) -> Vec<(String, Value)> {
    section["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| (d["id"].as_str().unwrap().to_string(), d["ready"].clone()))
        .collect()
}

#[tokio::test]
async fn an_item_shows_a_ready_draft_of_its_thread_until_the_draft_is_stale() {
    let dir = temp("ready-draft");
    let (store, _) = filled_n(&dir, 30);
    let person = sender(&store, "dana@contoso.example", Some("person"));
    let owe = json!({"direction": "you_owe", "kind": "task", "owner": {"kind": "self"},
                     "counterparty": party(&person)});
    // Two items from messages of two threads. Understanding and the link
    // check finished each message.
    exec(&store, "UPDATE message SET understood_at = 1 WHERE ?1", [1]);
    exec(&store, "DELETE FROM link_check WHERE ?1", [1]);
    let (first, other): (i64, i64) = store
        .read(|c| {
            Ok(c.query_row(
                "SELECT min(m.id), (SELECT min(o.id) FROM message o WHERE o.thread_id <> m.thread_id)
                 FROM message m WHERE m.thread_id = (SELECT thread_id FROM message WHERE id = 1)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap();
    let with = add_item(&store, "commitment", "open", first, 10, owe.clone());
    let without = add_item(&store, "commitment", "open", other, 20, owe);
    // The draft of the thread of `with` answers its newest message.
    exec(
        &store,
        "INSERT INTO reply_draft(thread_id, based_on, body, model, prompt, samples, created_at)
         SELECT m.thread_id, m.id, 'Yes.', 'fake', 'draft/0.1', 0, 1 FROM message m
         WHERE m.thread_id = (SELECT thread_id FROM message WHERE id = ?1)
         ORDER BY coalesce(m.internal_date, 0) DESC, m.id DESC LIMIT 1",
        [first],
    );
    // The text of the draft: GET /api/threads/{t}/draft.
    let thread = format!(
        "t-{}",
        store
            .read(|c| Ok(c.query_row(
                "SELECT thread_id FROM message WHERE id = ?1",
                [first],
                |r| r.get::<_, i64>(0)
            )?))
            .unwrap()
    );
    let (status, draft) = get(&store, &format!("/api/threads/{thread}/draft")).await;
    assert_eq!(status, StatusCode::OK, "{draft}");
    assert_eq!(draft["body"], "Yes.");
    assert_eq!(draft["nudge"], false);
    // A nudge whose thread has no late commitment is stale, on both paths.
    exec(
        &store,
        "UPDATE reply_draft SET ask = '{\"to\": \"x@x.example\"}' WHERE ?1",
        [1],
    );
    let (status, _) = get(&store, &format!("/api/threads/{thread}/draft")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (_, brief) = get(&store, "/api/brief").await;
    assert_eq!(brief["you_owe"]["items"][0]["ready"], Value::Null);
    exec(&store, "UPDATE reply_draft SET ask = NULL WHERE ?1", [1]);
    let (status, _) = get(&store, "/api/threads/m-1/draft").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let ready = json!(["draft"]);
    let want = vec![
        (format!("c-{with}"), ready.clone()),
        (format!("c-{without}"), Value::Null),
    ];
    let (_, brief) = get(&store, "/api/brief").await;
    assert_eq!(ready_marks(&brief["you_owe"]), want);
    let (_, page) = get(&store, "/api/brief/you_owe").await;
    assert_eq!(ready_marks(&page), want);
    // The open model allows the field: the item with it passes its schema.
    let schema = iris_spec::validator("commitment");
    let marked = &brief["you_owe"]["items"][0];
    assert!(schema.is_valid(marked), "{marked}");
    let mut wrong = marked.clone();
    wrong["ready"] = json!(["draft", "draft"]);
    assert!(!schema.is_valid(&wrong), "{wrong}");
    wrong["ready"] = json!(["dinner"]);
    assert!(!schema.is_valid(&wrong), "{wrong}");

    // A `ready` value in the stored document is never shown: the engine
    // computes the field on each read. Both items store a wrong value.
    exec(
        &store,
        "UPDATE item SET doc = json_set(doc, '$.ready', json('[\"draft\"]')) WHERE ?1",
        [1],
    );
    let (_, brief) = get(&store, "/api/brief").await;
    assert_eq!(ready_marks(&brief["you_owe"]), want);
    let (_, one) = get(&store, &format!("/api/items/c-{without}")).await;
    assert_eq!(one["ready"], Value::Null, "{one}");

    // Hostile mail in the thread: the draft is not ready. The newest message
    // of the thread is not the origin of the item, which a read leaves out
    // while it is hostile mail (issue 112).
    exec(
        &store,
        "UPDATE message SET hostile = '[\"spoof\"]'
         WHERE id = (SELECT based_on FROM reply_draft) AND id <> ?1",
        [first],
    );
    assert_eq!(
        store
            .read(|c| Ok(c.query_row(
                "SELECT count(*) FROM message WHERE hostile = '[\"spoof\"]'",
                [],
                |r| r.get::<_, i64>(0)
            )?))
            .unwrap(),
        1
    );
    let (_, brief) = get(&store, "/api/brief").await;
    assert_eq!(
        ready_marks(&brief["you_owe"]),
        vec![
            (format!("c-{with}"), Value::Null),
            (format!("c-{without}"), Value::Null),
        ]
    );
    let (status, _) = get(&store, &format!("/api/threads/{thread}/draft")).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "a stale draft");
}

/// R6-02: the brief shows the items of each account, each with the `a-` ID
/// of its account, and the item passes the schema of model 1.1.
#[tokio::test]
async fn the_brief_shows_the_items_of_each_account() {
    let dir = temp("two-accounts");
    let (store, _) = filled_n(&dir, 30);
    let second = store.add_account("gmail", "second@example.com").unwrap();
    let person = sender(&store, "dana@contoso.example", Some("person"));
    let owe = json!({"direction": "you_owe", "kind": "task", "owner": {"kind": "self"},
                     "counterparty": party(&person)});
    exec(&store, "UPDATE message SET understood_at = 1 WHERE ?1", [1]);
    exec(&store, "DELETE FROM link_check WHERE ?1", [1]);
    let first = add_item(&store, "commitment", "open", 1, 10, owe.clone());
    let other = add_item(&store, "commitment", "open", 2, 20, owe);
    exec(
        &store,
        "UPDATE item SET account_id = ?1 WHERE id = ?2",
        [second, other],
    );
    let (_, brief) = get(&store, "/api/brief").await;
    let items = brief["you_owe"]["items"].as_array().unwrap();
    let mut shown: Vec<(String, String)> = items
        .iter()
        .map(|i| {
            (
                i["id"].as_str().unwrap().into(),
                i["account"].as_str().unwrap().into(),
            )
        })
        .collect();
    shown.sort();
    assert_eq!(
        shown,
        vec![
            (format!("c-{first}"), "a-1".to_string()),
            (format!("c-{other}"), format!("a-{second}")),
        ]
    );
    let schema = iris_spec::validator("commitment");
    for item in items {
        assert_eq!(item["model"], iris_store::MODEL);
        assert!(schema.is_valid(item), "{item}");
    }
    let mut wrong = items[0].clone();
    wrong["account"] = json!("a-0");
    assert!(!schema.is_valid(&wrong), "{wrong}");
    // A field of 1.1 in a document that claims 1.0 is not valid (spec 4, rule 2.8).
    let mut old = items[0].clone();
    old["model"] = json!("iris/1.0");
    assert!(!schema.is_valid(&old), "{old}");
    // The status lists both accounts, so the client can name each one.
    let (_, status) = get(&store, "/api/status").await;
    let addresses: Vec<&str> = status["accounts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a["address"].as_str().unwrap())
        .collect();
    assert_eq!(addresses, ["owner@example.com", "second@example.com"]);
    // A removed account leaves the brief at once, and the status at the end
    // of its delete (docs/specs/2026-11-24-remove-account.md).
    // The delete derives each engine sender kind again from the evidence
    // that stays. This test has no evidence rows, so Dana is a decision of
    // the person, which no refresh changes.
    exec(
        &store,
        "UPDATE sender SET decided_by = 'person' WHERE ?1",
        [1],
    );
    // The route refuses an address that is not the address of the ID.
    let uri = format!("/api/accounts/a-{second}/remove");
    for (address, code) in [("owner@example.com", 400), ("other@example.com", 400)] {
        let (got, _) = post(&store, &uri, json!({ "address": address })).await;
        assert_eq!(got.as_u16(), code, "{address}");
    }
    let (got, _) = post(
        &store,
        "/api/accounts/a-x/remove",
        json!({ "address": "x" }),
    )
    .await;
    assert_eq!(got, StatusCode::BAD_REQUEST);
    let (_, status) = get(&store, "/api/status").await;
    assert_eq!(status["accounts"].as_array().unwrap().len(), 2);

    let (got, status) = post(&store, &uri, json!({ "address": "second@example.com" })).await;
    assert_eq!(got, StatusCode::OK);
    assert_eq!(status["accounts"].as_array().unwrap().len(), 1);
    let (_, brief) = get(&store, "/api/brief").await;
    let ids: Vec<&str> = brief["you_owe"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, [format!("c-{first}")]);
    // A second removal of the same account is a conflict.
    let (got, _) = post(&store, &uri, json!({ "address": "second@example.com" })).await;
    assert_eq!(got, StatusCode::CONFLICT);
}

async fn post(store: &Arc<Store>, uri: &str, body: Value) -> (StatusCode, Value) {
    let req = Request::post(uri)
        .header("host", "127.0.0.1:4800")
        .header("authorization", auth().bearer())
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap();
    let res = iris_api::router(store.clone(), auth())
        .oneshot(req)
        .await
        .unwrap();
    let code = res.status();
    let body = to_bytes(res.into_body(), 64 << 20).await.unwrap();
    (code, serde_json::from_slice(&body).unwrap_or(Value::Null))
}

/// Issue 740 (review of PR #786): `waits_model` counts only live mail that
/// the understanding step marked as waiting for a model. A draft, gone mail
/// and understood mail never count, so the client never says that mail
/// waits for a model when none does.
#[tokio::test]
async fn status_counts_only_live_mail_that_waits_for_a_model() {
    let dir = temp("waits-model");
    let (store, _) = filled_n(&dir, 6);
    let (_, v) = get(&store, "/api/status").await;
    assert_eq!(v["waits_model"], 0, "no mark yet");

    exec(&store, "UPDATE message SET waits_model = 1", []);
    exec(&store, "UPDATE message SET gone_at = 1 WHERE id = 1", []);
    exec(
        &store,
        "UPDATE message SET understood_at = 1 WHERE id = 2",
        [],
    );
    exec(
        &store,
        "INSERT INTO label(account_id, provider_id, name, kind) VALUES (1, 'DRAFT', 'DRAFT', 'system')",
        [],
    );
    exec(
        &store,
        "INSERT INTO message_label(message_id, label_id)
         SELECT 3, id FROM label WHERE provider_id = 'DRAFT'",
        [],
    );
    // The mark of a model error is not a missing model.
    exec(
        &store,
        "UPDATE message SET waits_model = 2 WHERE id = 4",
        [],
    );
    let (_, v) = get(&store, "/api/status").await;
    assert_eq!(v["waits_model"], 2, "messages 5 and 6 only");
}

/// Issue 112: an item whose origin message is hostile mail (a hostile
/// mark, the label `SPAM` or a hostile sender) is in no read of the API,
/// also when it stays in the store. It shows again when the state clears.
#[tokio::test]
async fn an_item_of_hostile_mail_is_hidden_until_the_state_clears() {
    let dir = temp("hostile-items");
    let (store, _) = filled_n(&dir, 4);
    store.put_labels(1, &[("SPAM", "SPAM", "system")]).unwrap();
    let feed = add_item(
        &store,
        "feed",
        "new",
        1,
        1,
        json!({"status": "new", "publisher": {"address": "news@example.com"},
               "published": "2026-11-01T00:00:00Z"}),
    );
    let record = add_item(&store, "record", "active", 1, 2, json!({"type": "receipt"}));
    // A record of another message stays in each read.
    let other = add_item(&store, "record", "active", 2, 3, json!({"type": "receipt"}));
    // The brief counts the hostile mail of the last 7 days.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    exec(
        &store,
        "UPDATE message SET internal_date = ?1 WHERE id = 1",
        [now],
    );
    let (f, r, o) = (
        format!("f-{feed}"),
        format!("r-{record}"),
        format!("r-{other}"),
    );
    let shown = hostile_reads(&store, &f, &r).await;
    assert!(
        shown.0.contains(&f) && shown.0.contains(&r) && shown.0.contains(&o),
        "{shown:?}"
    );
    assert_eq!(shown.1, vec![f.clone()]);
    assert_eq!(shown.2, json!({"receipt": 2}));
    assert_eq!((shown.3, shown.4), (StatusCode::OK, StatusCode::OK));
    assert_eq!(shown.5, json!([f.clone(), r.clone()]));
    assert_eq!(shown.6, 0);
    let states: [(&str, &str); 3] = [
        ("UPDATE message SET hostile = '[\"lookalike\"]' WHERE id = 1", "UPDATE message SET hostile = '[]' WHERE id = 1"),
        ("INSERT INTO message_label(message_id, label_id) SELECT 1, id FROM label WHERE provider_id = 'SPAM'",
         "DELETE FROM message_label WHERE message_id = 1 AND label_id = (SELECT id FROM label WHERE provider_id = 'SPAM')"),
        ("INSERT INTO sender(address, kind) SELECT from_addr, 'hostile' FROM header WHERE message_id = 1
            ON CONFLICT(address) DO UPDATE SET kind = 'hostile'",
         "UPDATE sender SET kind = 'person' WHERE address = (SELECT from_addr FROM header WHERE message_id = 1)"),
    ];
    for (set, clear) in states {
        exec(&store, set, []);
        let hidden = hostile_reads(&store, &f, &r).await;
        assert!(
            !hidden.0.contains(&f) && !hidden.0.contains(&r) && hidden.0.contains(&o),
            "{set}: {hidden:?}"
        );
        assert!(hidden.1.is_empty(), "{set}: {hidden:?}");
        assert_eq!(hidden.2, json!({"receipt": 1}), "{set}");
        assert_eq!(
            (hidden.3, hidden.4),
            (StatusCode::NOT_FOUND, StatusCode::NOT_FOUND),
            "{set}"
        );
        assert_eq!(hidden.5, json!([]), "{set}");
        // Each state counts as hostile mail of the brief (spec 1, section 8).
        assert_eq!(hidden.6, 1, "{set}");
        exec(&store, clear, []);
        assert_eq!(hostile_reads(&store, &f, &r).await, shown, "{clear}");
    }
}

/// The reads of the hostile item test: the IDs of `/api/items`, the IDs of
/// the feed of the brief, the record counts of the brief, and the status of
/// `/api/items/{feed}` and `/api/items/{record}`, and the items of the mail
/// view of the origin message, and the count of hostile mail of the brief.
async fn hostile_reads(
    store: &Arc<Store>,
    feed: &str,
    record: &str,
) -> (
    Vec<String>,
    Vec<String>,
    Value,
    StatusCode,
    StatusCode,
    Value,
    Value,
) {
    let ids_of = |v: &Value| -> Vec<String> {
        v["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|d| d["id"].as_str().unwrap().to_string())
            .collect()
    };
    let (_, items) = get(store, "/api/items?limit=100").await;
    let (_, brief) = get(store, "/api/brief").await;
    (
        ids_of(&items),
        ids_of(&brief["feed"]),
        brief["records"].clone(),
        get(store, &format!("/api/items/{feed}")).await.0,
        get(store, &format!("/api/items/{record}")).await.0,
        get(store, "/api/messages/m-1").await.1["items"].clone(),
        brief["hostile"].clone(),
    )
}

/// Decision D1: the status gives the time zone of the owner, so the client
/// offers the days of Later in that zone.
#[tokio::test]
async fn status_gives_the_time_zone_of_the_owner() {
    let dir = temp("time-zone");
    let (store, _) = filled_n(&dir, 1);
    let (_, v) = get(&store, "/api/status").await;
    let computer = jiff::tz::TimeZone::system();
    assert_eq!(v["time_zone"], serde_json::json!(computer.iana_name()));
    assert_eq!(v["time_zone_set"], false);
    store.set_time_zone(Some("Asia/Kolkata")).unwrap();
    let (_, v) = get(&store, "/api/status").await;
    assert_eq!(v["time_zone"], "Asia/Kolkata");
    assert_eq!(v["time_zone_set"], true);
}

/// Issue 891: the web client sets the time zone of the owner, as `iris
/// zone` does. A name that the computer does not know, a missing key and
/// another key are a 400, and nothing is written. Null goes back to the
/// time zone of the computer.
#[tokio::test]
async fn the_person_sets_the_time_zone_from_the_web_client() {
    let dir = temp("zone-set");
    let (store, _) = filled_n(&dir, 1);
    let computer = serde_json::json!(jiff::tz::TimeZone::system().iana_name());

    let (code, v) = post(
        &store,
        "/api/zone",
        serde_json::json!({ "zone": "Asia/Kolkata" }),
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(
        v,
        serde_json::json!({ "time_zone": "Asia/Kolkata", "time_zone_set": true })
    );
    let (_, s) = get(&store, "/api/status").await;
    assert_eq!(s["time_zone"], "Asia/Kolkata");

    for bad in [
        serde_json::json!({ "zone": "Mars/Olympus" }),
        serde_json::json!({ "zone": "" }),
        serde_json::json!({ "zone": 5 }),
        serde_json::json!({}),
        serde_json::json!({ "zone": "Europe/Berlin", "more": 1 }),
    ] {
        let (code, _) = post(&store, "/api/zone", bad.clone()).await;
        assert_eq!(code, StatusCode::BAD_REQUEST, "{bad}");
        let (_, s) = get(&store, "/api/status").await;
        assert_eq!(s["time_zone"], "Asia/Kolkata", "nothing written for {bad}");
    }

    let (code, v) = post(&store, "/api/zone", serde_json::json!({ "zone": null })).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(
        v,
        serde_json::json!({ "time_zone": computer, "time_zone_set": false })
    );
}

/// Issue 891 review: the status reads the time zone and `time_zone_set`
/// from one read, so a change of the zone at the same time never gives a
/// mixed pair (the old zone with the new flag, or the reverse).
#[test]
fn the_time_zone_and_its_flag_always_agree_while_the_zone_changes() {
    let dir = temp("zone-pair");
    let (store, _) = filled_n(&dir, 1);
    let computer = jiff::tz::TimeZone::system().iana_name().map(str::to_string);
    // A zone that is not the zone of the computer, so the two states differ.
    let zone = ["Asia/Kolkata", "Pacific/Chatham"]
        .into_iter()
        .find(|z| computer.as_deref() != Some(*z))
        .unwrap();
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let writer = {
        let (store, stop) = (store.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut on = false;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                on = !on;
                store.set_time_zone(on.then_some(zone)).unwrap();
            }
        })
    };
    for _ in 0..300 {
        let s = iris_api::status(&store).unwrap();
        let want = if s.time_zone_set {
            Some(zone.to_string())
        } else {
            computer.clone()
        };
        assert_eq!(s.time_zone, want, "set = {}", s.time_zone_set);
        let z = iris_api::zone(&store).unwrap();
        let want = if z["time_zone_set"] == true {
            serde_json::json!(zone)
        } else {
            serde_json::json!(computer)
        };
        assert_eq!(z["time_zone"], want, "{z}");
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    writer.join().unwrap();
}
