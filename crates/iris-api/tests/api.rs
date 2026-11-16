//! Tests of the local API on a store filled from the synthetic mailbox.

use std::path::PathBuf;
use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{header, Request, StatusCode};
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
    let store = Store::open(&dir.0).unwrap();
    let acc = store.add_account("gmail", "owner@example.com").unwrap();
    store
        .put_labels(
            acc,
            &[("INBOX", "INBOX", "system"), ("SENT", "SENT", "system")],
        )
        .unwrap();
    let mailbox = Mailbox::new(11, COUNT);
    let mut batch = Vec::new();
    for i in 0..COUNT {
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

/// Add an item with `doc` (merged over the common fields), with message
/// row `source` as its origin. Gives the row ID.
fn add_item(store: &Store, form: &str, status: &str, source: i64, created: i64, doc: Value) -> i64 {
    let mut full = json!({
        "model": "iris/0",
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

async fn call(
    store: &Arc<Store>,
    uri: &str,
    headers: &[(&str, &str)],
) -> (StatusCode, Vec<u8>, axum::http::HeaderMap) {
    let mut req = Request::get(uri);
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let res = iris_api::router(store.clone())
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
async fn refuses_a_host_or_origin_that_is_not_local() {
    let dir = temp("host");
    let store = Arc::new(Store::open(&dir.0).unwrap());
    let cases: [(&[(&str, &str)], StatusCode); 6] = [
        (&[("host", "localhost:4800")], StatusCode::OK),
        (
            &[("host", "[::1]:4800"), ("origin", "http://localhost:5173")],
            StatusCode::OK,
        ),
        (&[("host", "evil.example")], StatusCode::FORBIDDEN),
        (
            &[("host", "127.0.0.1.evil.example:4800")],
            StatusCode::FORBIDDEN,
        ),
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
    assert_eq!(v["messages"]["total"], 0);

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
    let (_, v) = get(&store, "/api/status").await;
    let a = &v["accounts"][0];
    assert_eq!(a["address"], "owner@example.com");
    assert_eq!(a["mirror_ready_at"], Value::Null);
    assert_eq!(a["sync"], json!({"kind": "first", "step": 2}));
    assert_eq!(a["last_sync"]["started"], "1970-01-01T00:00:00Z");
    assert_eq!(a["last_sync"]["ended"], Value::Null);
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
    assert_eq!(m["items"], json!([format!("f-{item}")]));

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
    let (code, body, h) = call(&store, "/api/messages/m-3/raw", &[("host", "localhost")]).await;
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
    let r1 = add_item(
        &store,
        "record",
        "kept",
        1,
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
    assert_eq!(ids(&p["items"]), [format!("r-{r3}"), format!("r-{r2}")]);
    assert_eq!(p["next"], format!("r-{r2}"));
    let (_, p) = get(
        &store,
        &format!("/api/items?form=record&limit=2&before=r-{r2}"),
    )
    .await;
    assert_eq!(ids(&p["items"]), [format!("r-{r1}")]);
    assert_eq!(p["next"], Value::Null);

    // Filters.
    let (_, p) = get(&store, "/api/items?form=record&type=receipt").await;
    assert_eq!(ids(&p["items"]), [format!("r-{r3}"), format!("r-{r1}")]);
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
        "/api/items?limit=201",
        "/api/search?q=a&limit=0",
    ] {
        assert_eq!(get(&store, bad).await.0, StatusCode::BAD_REQUEST, "{bad}");
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
    assert_eq!(ids(&b["feed"]["items"]), [f(f_newer), f(f_new), f(f_other)]);
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

    // The HTTP route gives the same shape.
    let (code, http) = get(&store, "/api/brief").await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(http["records"], b["records"]);
}

#[tokio::test]
async fn merges_with_the_web_client_router() {
    let dir = temp("merge");
    let store = Arc::new(Store::open(&dir.0).unwrap());
    let app = iris_api::router(store).merge(iris_web::router());
    let send = |uri: &str| {
        let req = Request::get(uri)
            .header("host", "127.0.0.1:4800")
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
