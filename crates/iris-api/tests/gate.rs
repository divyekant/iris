//! Tests of the stranger gate (specification 1, section 6.7): `GET
//! /api/gate`, `GET /api/gate/declined`, `POST /api/senders/{id}/{answer}`
//! and the `gate` and `declined` fields of the brief.

use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::http::{Request, StatusCode};
use iris_api::Auth;
use iris_store::{Header, NewMessage, Store};
use serde_json::{json, Value};
use tower::ServiceExt;

struct Gate {
    store: Arc<Store>,
    auth: Arc<Auth>,
    dir: std::path::PathBuf,
}

impl Drop for Gate {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// Message `n` (1 to 9) is from `from`, at `n` seconds.
const FROM: [&str; 9] = [
    "ana@paypal.com",
    "ana@paypal.com",
    "bo@later.example",
    "dee@contoso.example",
    "alerts@paypa1.com",
    "cy@nowhere.example",
    "cy@nowhere.example",
    "cy@nowhere.example",
    "shop@shop.example",
];

impl Gate {
    fn new(name: &str) -> Gate {
        let dir = std::env::temp_dir().join(format!("iris-api-gate-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::open(&dir).unwrap();
        let a = store.add_account("gmail", "owner@example.com").unwrap();
        store
            .put_labels(a, &[("INBOX", "INBOX", "system")])
            .unwrap();
        let batch: Vec<NewMessage> = FROM
            .iter()
            .enumerate()
            .map(|(i, from)| NewMessage {
                provider_id: format!("g{i}"),
                thread_provider_id: format!("gt{i}"),
                internal_date: Some((i as i64 + 1) * 1000),
                labels: vec!["INBOX".into()],
                header: Header {
                    from_addr: Some(from.to_string()),
                    ..Default::default()
                },
                ..Default::default()
            })
            .collect();
        store.put_messages(a, &batch).unwrap();
        store
            .write(|tx| {
                tx.execute("UPDATE message SET understood_at = 1", [])?;
                Ok(())
            })
            .unwrap();
        Gate {
            store: Arc::new(store),
            auth: Arc::new(Auth::new("127.0.0.1:4800".parse().unwrap()).unwrap()),
            dir,
        }
    }

    fn exec(&self, sql: &str, args: impl rusqlite::Params) -> i64 {
        self.store
            .write(|tx| {
                tx.execute(sql, args)?;
                Ok(tx.last_insert_rowid())
            })
            .unwrap()
    }

    fn one<T: rusqlite::types::FromSql>(&self, sql: &str) -> T {
        self.store
            .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
            .unwrap()
    }

    /// A sender row with the kind `kind`. Gives the `p-` ID.
    fn sender(&self, address: &str, kind: Option<&str>) -> String {
        let id = self.exec(
            "INSERT INTO sender(address, kind, decided_by) VALUES (?1, ?2, 'engine')",
            rusqlite::params![address, kind],
        );
        format!("p-{id}")
    }

    /// An item of message `source`. Gives the item ID.
    fn item(&self, form: &str, status: &str, source: i64, doc: Value) -> String {
        let mut full = json!({
            "model": "iris/1.0", "form": form, "status": status, "title": format!("{form} {source}"),
            "sources": [{"message": format!("m-{source}"), "thread": "t-1", "role": "origin", "quote": "q"}],
            "created": "2026-11-01T00:00:00Z", "updated": "2026-11-01T00:00:00Z",
            "derived": {"by": "rule", "rule": "test"}
        });
        for (k, v) in doc.as_object().unwrap() {
            full[k] = v.clone();
        }
        let id = self.exec(
            "INSERT INTO item(form, account_id, status, doc, derived_by, created_at, updated_at)
             VALUES (?1, 1, ?2, ?3, 'rule', 0, 0)",
            rusqlite::params![form, status, full.to_string()],
        );
        self.exec(
            "INSERT INTO item_source(item_id, message_id, role) VALUES (?1, ?2, 'origin')",
            [id, source],
        );
        format!("{}-{id}", if form == "commitment" { "c" } else { "v" })
    }

    async fn call(&self, method: &str, uri: &str) -> (StatusCode, Value) {
        let req = Request::builder()
            .method(method)
            .uri(uri)
            .header("host", "127.0.0.1:4800")
            .header("authorization", self.auth.bearer())
            .body(Body::empty())
            .unwrap();
        let res = iris_api::router(self.store.clone(), self.auth.clone())
            .oneshot(req)
            .await
            .unwrap();
        let code = res.status();
        let body = to_bytes(res.into_body(), 16 << 20).await.unwrap();
        (code, serde_json::from_slice(&body).unwrap_or(Value::Null))
    }

    async fn get(&self, uri: &str) -> Value {
        let (code, v) = self.call("GET", uri).await;
        assert_eq!(code, StatusCode::OK, "{uri}: {v}");
        v
    }

    async fn answer(&self, sender: &str, answer: &str) -> (StatusCode, Value) {
        self.call("POST", &format!("/api/senders/{sender}/{answer}"))
            .await
    }
}

fn party(p: &str) -> Value {
    json!({"kind": "sender", "sender": p})
}

fn owe(p: &str) -> Value {
    json!({"direction": "you_owe", "kind": "task", "owner": {"kind": "self"}, "counterparty": party(p)})
}

fn talk(people: &[&str], at: &str) -> Value {
    let mut list = vec![json!({"kind": "self"})];
    list.extend(people.iter().map(|p| party(p)));
    json!({"participants": list, "role": "group", "messages": 2, "new": 1, "your_turn": true,
           "latest": {"at": at, "from": party(people[0]), "snippet": "hi"}})
}

/// The IDs of a list of documents.
fn ids(list: &Value) -> Vec<String> {
    list.as_array()
        .unwrap()
        .iter()
        .map(|d| d["id"].as_str().unwrap().to_string())
        .collect()
}

/// The senders of a gate page: (ID, count, item IDs).
fn senders(page: &Value) -> Vec<(String, i64, Vec<String>)> {
    page["senders"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["id"].as_str().unwrap().to_string(),
                s["count"].as_i64().unwrap(),
                ids(&s["items"]),
            )
        })
        .collect()
}

/// Each item ID in each list section of a brief.
fn in_sections(b: &Value) -> Vec<String> {
    [
        "you_owe",
        "owed_to_you",
        "conversations",
        "feed",
        "signals",
        "strangers",
    ]
    .iter()
    .flat_map(|s| ids(&b[s]["items"]))
    .collect()
}

/// The sample: Ana (stranger) with an ask and a group talk with Bo, Bo (no
/// kind yet, so a stranger) owes you a reply, Dee is a person, and Cy has
/// four asks for the page test.
struct Sample {
    ana: String,
    bo: String,
    dee: String,
    ask: String,
    group: String,
    reply: String,
    dee_ask: String,
}

fn sample(g: &Gate) -> Sample {
    let ana = g.sender("ana@paypal.com", Some("stranger"));
    let bo = g.sender("bo@later.example", None);
    let dee = g.sender("dee@contoso.example", Some("person"));
    let ask = g.item("commitment", "open", 1, owe(&ana));
    let group = g.item(
        "conversation",
        "active",
        2,
        talk(&[&ana, &bo], "1970-01-01T00:00:30Z"),
    );
    let reply = g.item(
        "commitment",
        "open",
        3,
        json!({"direction": "owed_to_you", "kind": "reply", "owner": party(&bo), "counterparty": {"kind": "self"}}),
    );
    let dee_ask = g.item("commitment", "open", 4, owe(&dee));
    Sample {
        ana,
        bo,
        dee,
        ask,
        group,
        reply,
        dee_ask,
    }
}

#[tokio::test]
async fn the_gate_is_the_strangers_section_by_sender() {
    let g = Gate::new("list");
    let s = sample(&g);
    let b = g.get("/api/brief").await;
    assert_eq!(b["strangers"]["count"], 3);
    assert_eq!(b["declined"], 0);
    assert_eq!(b["gate"]["count"], 2);
    // Newest first: the group talk (30 s) makes Ana the newest; Bo's newest
    // item is the same talk, so the sender ID decides.
    assert_eq!(
        senders(&b["gate"]),
        vec![
            (s.bo.clone(), 2, vec![s.group.clone(), s.reply.clone()]),
            (s.ana.clone(), 2, vec![s.group.clone(), s.ask.clone()]),
        ]
    );
    assert_eq!(b["gate"]["senders"][1]["address"], "ana@paypal.com");
    assert_eq!(b["gate"]["next"], Value::Null);
    assert_eq!(g.get("/api/gate").await, b["gate"]);
    assert_eq!(ids(&b["you_owe"]["items"]), vec![s.dee_ask.clone()]);

    // A rule decides first: a hidden item is not at the gate.
    g.exec(
        "UPDATE item SET doc = json_set(doc, '$.attention', json('{\"state\":\"hidden\",\"rule\":1}'))
          WHERE id IN (2, 3)",
        [],
    );
    let gate = g.get("/api/gate").await;
    assert_eq!(
        senders(&gate),
        vec![(s.ana.clone(), 1, vec![s.ask.clone()])]
    );
    // Bo is not at the gate now: no answer, and no write.
    for answer in ["accept", "decline"] {
        let (code, v) = g.answer(&s.bo, answer).await;
        assert_eq!(code, StatusCode::CONFLICT, "{answer}");
        assert!(
            v["error"].as_str().unwrap().contains("not at the gate"),
            "{v}"
        );
    }
    assert_eq!(
        g.one::<String>(&format!(
            "SELECT decided_by || ' ' || coalesce(declined_at, 'none') FROM sender WHERE id = {}",
            &s.bo[2..]
        )),
        "engine none"
    );
}

#[tokio::test]
async fn accept_moves_the_items_and_reopen_brings_them_back() {
    let g = Gate::new("accept");
    let s = sample(&g);
    let (code, v) = g.answer(&s.bo, "accept").await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(
        (
            v["id"].as_str(),
            v["kind"].as_str(),
            v["decided_by"].as_str(),
            v["declined"].as_bool()
        ),
        (
            Some(s.bo.as_str()),
            Some("person"),
            Some("person"),
            Some(false)
        )
    );
    let b = g.get("/api/brief").await;
    // Bo's reply is owed to you now. The talk still has Ana, a stranger.
    assert_eq!(ids(&b["owed_to_you"]["items"]), vec![s.reply.clone()]);
    assert_eq!(
        senders(&b["gate"]),
        vec![(s.ana.clone(), 2, vec![s.group.clone(), s.ask.clone()])]
    );
    // A second accept is no change.
    assert_eq!(g.answer(&s.bo, "accept").await.0, StatusCode::OK);

    g.answer(&s.ana, "accept").await;
    let b = g.get("/api/brief").await;
    assert_eq!(b["gate"]["count"], 0);
    assert_eq!(b["strangers"]["count"], 0);
    let mut owe = ids(&b["you_owe"]["items"]);
    owe.sort();
    let mut want = vec![s.ask.clone(), s.dee_ask.clone()];
    want.sort();
    assert_eq!(owe, want);
    assert_eq!(ids(&b["conversations"]["items"]), vec![s.group.clone()]);

    let (code, v) = g.answer(&s.ana, "reopen").await;
    assert_eq!(
        (code, v["kind"].as_str(), v["decided_by"].as_str()),
        (StatusCode::OK, Some("stranger"), Some("engine"))
    );
    let b = g.get("/api/brief").await;
    assert_eq!(senders(&b["gate"])[0].0, s.ana);
}

#[tokio::test]
async fn decline_takes_the_items_out_of_the_brief() {
    let g = Gate::new("decline");
    let s = sample(&g);
    let (code, v) = g.answer(&s.ana, "decline").await;
    assert_eq!(
        (code, v["declined"].as_bool()),
        (StatusCode::OK, Some(true)),
        "{v}"
    );
    let b = g.get("/api/brief").await;
    assert_eq!(b["declined"], 1);
    // The talk waits for Bo; the ask of Ana is in no section.
    assert_eq!(
        senders(&b["gate"]),
        vec![(s.bo.clone(), 2, vec![s.group.clone(), s.reply.clone()])]
    );
    assert!(!in_sections(&b).contains(&s.ask));
    let declined = g.get("/api/gate/declined").await;
    assert_eq!(
        senders(&declined),
        vec![(s.ana.clone(), 1, vec![s.ask.clone()])]
    );

    // Bo declined too: the talk leaves the brief, also the conversations.
    g.answer(&s.bo, "decline").await;
    let b = g.get("/api/brief").await;
    assert_eq!(
        (b["gate"]["count"].as_i64(), b["declined"].as_i64()),
        (Some(0), Some(2))
    );
    assert_eq!(in_sections(&b), vec![s.dee_ask.clone()]);
    assert_eq!(g.get("/api/gate/declined").await["count"], 2);

    // A declined sender can still be accepted, and reopen ends a decline.
    g.answer(&s.bo, "accept").await;
    assert_eq!(
        ids(&g.get("/api/brief").await["owed_to_you"]["items"]),
        vec![s.reply.clone()]
    );
    g.answer(&s.ana, "reopen").await;
    let b = g.get("/api/brief").await;
    assert_eq!(b["declined"], 0);
    assert_eq!(b["gate"]["count"], 1);
}

/// Decision D30 (issue 264): a decline of one sender does not hide the
/// thread of a known person.
#[tokio::test]
async fn a_group_talk_with_a_known_person_and_a_declined_sender_stays_in_conversations() {
    let g = Gate::new("d30");
    let s = sample(&g);
    let with_dee = g.item(
        "conversation",
        "active",
        5,
        talk(&[&s.ana, &s.dee], "1970-01-01T00:00:40Z"),
    );
    // Ana waits at the gate: the talk is under Strangers only.
    let b = g.get("/api/brief").await;
    assert!(ids(&b["strangers"]["items"]).contains(&with_dee));
    assert!(!ids(&b["conversations"]["items"]).contains(&with_dee));

    g.answer(&s.ana, "decline").await;
    let b = g.get("/api/brief").await;
    assert_eq!(ids(&b["conversations"]["items"]), vec![with_dee.clone()]);
    // The talk with Bo, a stranger with no known person, still waits.
    assert!(ids(&b["strangers"]["items"]).contains(&s.group));

    // A declined sender whose only item is such a talk stays on the list of
    // declined senders, so "Accept" and "Ask again" still work.
    let eve = g.sender("eve@far.example", Some("stranger"));
    let only = g.item(
        "conversation",
        "active",
        6,
        talk(&[&eve, &s.dee], "1970-01-01T00:00:50Z"),
    );
    assert_eq!(g.answer(&eve, "decline").await.0, StatusCode::OK);
    let b = g.get("/api/brief").await;
    assert!(ids(&b["conversations"]["items"]).contains(&only));
    let declined = g.get("/api/gate/declined").await;
    let eve_row = senders(&declined)
        .into_iter()
        .find(|(id, _, _)| *id == eve)
        .expect("eve is on the list of declined senders");
    assert_eq!(eve_row.2, vec![only.clone()]);
    let (code, v) = g.answer(&eve, "accept").await;
    assert_eq!(
        (code, v["kind"].as_str()),
        (StatusCode::OK, Some("person")),
        "{v}"
    );
}

#[tokio::test]
async fn the_gate_refuses_what_is_not_a_stranger() {
    let g = Gate::new("refuse");
    let s = sample(&g);
    // Alice is known in paypal.com: Ana is no look-alike, but paypa1.com is.
    g.sender("alice@paypal.com", Some("person"));
    let look = g.sender("alerts@paypa1.com", Some("stranger"));
    let (code, v) = g.answer(&look, "accept").await;
    assert_eq!(code, StatusCode::CONFLICT);
    assert!(v["error"].as_str().unwrap().contains("hostile"), "{v}");
    let (code, _) = g.answer(&s.dee, "decline").await;
    assert_eq!(code, StatusCode::CONFLICT);
    assert_eq!(g.answer("p-999", "accept").await.0, StatusCode::NOT_FOUND);
    assert_eq!(g.answer(&s.ana, "block").await.0, StatusCode::NOT_FOUND);
    assert_eq!(g.answer("c-1", "accept").await.0, StatusCode::BAD_REQUEST);
    assert_eq!(
        g.call("GET", &format!("/api/senders/{}/accept", s.ana))
            .await
            .0,
        StatusCode::METHOD_NOT_ALLOWED
    );
    // Nothing changed.
    assert_eq!(
        g.one::<Option<String>>(
            "SELECT decided_by FROM sender WHERE address = 'alerts@paypa1.com'"
        )
        .as_deref(),
        Some("engine")
    );
}

#[tokio::test]
async fn accept_runs_understanding_again_on_a_new_look_alike() {
    let g = Gate::new("rerun");
    let s = sample(&g);
    let look = g.sender("alerts@paypa1.com", Some("stranger"));
    let look_ask = g.item("commitment", "open", 5, owe(&look));
    assert_eq!(g.get("/api/gate").await["count"], 3);
    g.answer(&s.ana, "accept").await;
    // paypal.com is known now: the look-alike is hostile, its item is gone,
    // and its message waits for understanding again.
    assert_eq!(
        g.one::<String>("SELECT kind FROM sender WHERE address = 'alerts@paypa1.com'"),
        "hostile"
    );
    let row: i64 = look_ask[2..].parse().unwrap();
    assert_eq!(
        g.one::<i64>(&format!("SELECT count(*) FROM item WHERE id = {row}")),
        0
    );
    assert_eq!(
        g.one::<Option<i64>>("SELECT understood_at FROM message WHERE id = 5"),
        None
    );
    assert_eq!(
        g.one::<Option<i64>>("SELECT understood_at FROM message WHERE id = 1"),
        Some(1)
    );
    assert_eq!(g.get("/api/gate").await["count"], 1);
}

#[tokio::test]
async fn the_gate_pages_by_sender() {
    let g = Gate::new("pages");
    sample(&g);
    let cy = g.sender("cy@nowhere.example", Some("stranger"));
    let shop = g.sender("shop@shop.example", Some("stranger"));
    let mut asks: Vec<String> = (6..=8)
        .map(|m| g.item("commitment", "open", m, owe(&cy)))
        .collect();
    asks.push(g.item("commitment", "open", 8, owe(&cy)));
    g.item("commitment", "open", 9, owe(&shop));

    let mut seen = Vec::new();
    let mut uri = "/api/gate?limit=2".to_string();
    loop {
        let page = g.get(&uri).await;
        assert_eq!(page["count"], 4);
        let list = senders(&page);
        assert!(list.len() <= 2);
        seen.extend(list);
        match page["next"].as_str() {
            Some(n) => uri = format!("/api/gate?limit=2&after={n}"),
            None => break,
        }
    }
    let order: Vec<&str> = seen.iter().map(|s| s.0.as_str()).collect();
    // Bo and Ana share the talk at 30 s, then Shop (9 s) and Cy (8 s).
    assert_eq!(order, vec!["p-2", "p-1", shop.as_str(), cy.as_str()]);
    // Cy has four asks: the page gives the count and the newest three.
    assert_eq!(seen[3].1, 4);
    assert_eq!(seen[3].2.len(), 3);
    assert!(
        !seen[3].2.contains(&asks[0]),
        "the oldest ask is not in the page"
    );

    for bad in [
        "/api/gate?limit=0",
        "/api/gate?limit=201",
        "/api/gate?after=zz",
        "/api/gate/declined?after=00",
    ] {
        assert_eq!(g.call("GET", bad).await.0, StatusCode::BAD_REQUEST, "{bad}");
    }
    // A cursor of one list is not a cursor of the other.
    let next = g.get("/api/gate?limit=1").await["next"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(
        g.call("GET", &format!("/api/gate/declined?after={next}"))
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
}

/// The state of a sender in the model of the generated test.
#[derive(Clone, Copy, PartialEq, Debug)]
enum State {
    Known,
    Waiting,
    Declined,
}

#[tokio::test]
async fn each_gate_item_is_in_one_place_after_any_answers() {
    // Invariant (spec 1 sections 6.7 and 8): an item with a stranger that
    // waits is in `strangers` and under each such sender at the gate; an
    // item with a declined stranger and none that waits is in no section and
    // under each such sender in the declined list; else the item is in its
    // own section. This holds after each answer of the person.
    const DOMAINS: [&str; 5] = ["north", "quartz", "violet", "harbor", "meadow"];
    for seed in 1..=8u64 {
        let g = Gate::new(&format!("prop-{seed}"));
        let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
        let mut next = move |n: u64| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x % n
        };
        let mut ids = Vec::new();
        let mut state = Vec::new();
        for d in DOMAINS {
            let (kind, s) = match next(4) {
                0 => (Some("person"), State::Known),
                1 => (None, State::Waiting),
                _ => (Some("stranger"), State::Waiting),
            };
            ids.push(g.sender(&format!("x@{d}.example"), kind));
            state.push(s);
        }
        // (item ID, form, parties as sender indexes, direction).
        let mut items: Vec<(String, &str, Vec<usize>)> = Vec::new();
        for i in 0..24 {
            let source = 1 + (i % 9) as i64;
            let a = next(5) as usize;
            let item = match next(3) {
                0 => (g.item("commitment", "open", source, owe(&ids[a])), "you_owe", vec![a]),
                1 => (
                    g.item(
                        "commitment",
                        "open",
                        source,
                        json!({"direction": "owed_to_you", "kind": "reply", "owner": party(&ids[a]), "counterparty": {"kind": "self"}}),
                    ),
                    "owed_to_you",
                    vec![a],
                ),
                _ => {
                    let mut people = vec![a];
                    for _ in 0..next(3) {
                        let b = next(5) as usize;
                        if !people.contains(&b) {
                            people.push(b);
                        }
                    }
                    let list: Vec<&str> = people.iter().map(|p| ids[*p].as_str()).collect();
                    (g.item("conversation", "active", source, talk(&list, "1970-01-01T00:00:10Z")), "conversations", people)
                }
            };
            items.push(item);
        }

        for step in 0..12 {
            // The model and the API agree.
            let b = g.get("/api/brief").await;
            let gate = g.get("/api/gate?limit=200").await;
            let declined = g.get("/api/gate/declined?limit=200").await;
            let mut want_gate: Vec<(String, i64)> = Vec::new();
            let mut want_declined: Vec<(String, i64)> = Vec::new();
            for (i, id) in ids.iter().enumerate() {
                let n = items.iter().filter(|(_, _, p)| p.contains(&i)).count() as i64;
                let waits = |p: &Vec<usize>| p.iter().any(|q| state[*q] == State::Waiting);
                match state[i] {
                    State::Waiting if n > 0 => want_gate.push((id.clone(), n)),
                    State::Declined => {
                        // A conversation with a known person is in
                        // Conversations and on this list too (decision D30).
                        let n = items
                            .iter()
                            .filter(|(_, _, p)| p.contains(&i) && !waits(p))
                            .count() as i64;
                        if n > 0 {
                            want_declined.push((id.clone(), n));
                        }
                    }
                    _ => {}
                }
            }
            let got = |page: &Value| {
                let mut v: Vec<(String, i64)> = senders(page)
                    .into_iter()
                    .map(|(id, n, _)| (id, n))
                    .collect();
                v.sort();
                v
            };
            want_gate.sort();
            want_declined.sort();
            assert_eq!(got(&gate), want_gate, "seed {seed} step {step} gate");
            assert_eq!(
                got(&declined),
                want_declined,
                "seed {seed} step {step} declined"
            );
            assert_eq!(gate, b["gate"], "seed {seed} step {step}");
            assert_eq!(b["declined"].as_i64().unwrap(), want_declined.len() as i64);
            for (id, section, people) in &items {
                let has = |s: State| people.iter().any(|p| state[*p] == s);
                let place = if has(State::Waiting) {
                    Some("strangers")
                } else if has(State::Declined)
                    && !(*section == "conversations" && has(State::Known))
                {
                    None
                } else if *section == "conversations"
                    || people.iter().all(|p| state[*p] == State::Known)
                {
                    Some(*section)
                } else {
                    None
                };
                let found: Vec<&str> = ["you_owe", "owed_to_you", "conversations", "strangers"]
                    .into_iter()
                    .filter(|s| ids_of(&b[*s]["items"]).contains(id))
                    .collect();
                assert_eq!(
                    found,
                    place.into_iter().collect::<Vec<_>>(),
                    "seed {seed} step {step} {id}"
                );
            }

            // One answer of the person, and its effect on the model.
            let who = next(5) as usize;
            let answer = ["accept", "decline", "reopen"][next(3) as usize];
            let (code, v) = g.answer(&ids[who], answer).await;
            let before = state[who];
            let after = match (answer, before) {
                // A second accept is no change; a known sender is not at the gate.
                ("accept", State::Known) => {
                    let again = code == StatusCode::OK && v["decided_by"] == "person";
                    assert!(again || code == StatusCode::CONFLICT, "{code} {v}");
                    before
                }
                ("decline", State::Known) => {
                    assert_eq!(code, StatusCode::CONFLICT, "{v}");
                    before
                }
                ("accept", _) => State::Known,
                ("decline", _) => State::Declined,
                // Reopen: the engine decides again. With no evidence, a row
                // that the person accepted is a stranger; a row that the
                // engine made `person` keeps its kind.
                _ if v["kind"] == "person" => State::Known,
                _ => State::Waiting,
            };
            if code == StatusCode::OK {
                assert_eq!(v["id"], ids[who].as_str());
            }
            state[who] = after;
        }
    }
}

fn ids_of(list: &Value) -> Vec<String> {
    ids(list)
}

#[tokio::test]
async fn a_gate_sender_with_the_name_of_a_known_person_says_so() {
    // Issue #370: a new address under the name of a known person.
    let g = Gate::new("known-name");
    let s = sample(&g);
    g.exec(
        "UPDATE sender SET name = 'Dee Ray' WHERE address = 'dee@contoso.example'",
        [],
    );
    g.exec(
        "UPDATE sender SET name = 'dee RAY' WHERE address = 'ana@paypal.com'",
        [],
    );
    g.exec(
        "UPDATE sender SET name = '' WHERE address = 'bo@later.example'",
        [],
    );
    // A stranger with the same name is not a known person.
    g.sender("eve@other.example", Some("stranger"));
    g.exec(
        "UPDATE sender SET name = '' WHERE address = 'eve@other.example'",
        [],
    );
    let gate = g.get("/api/gate").await;
    let of = |id: &str| {
        gate["senders"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["id"] == id)
            .unwrap()["known_as"]
            .clone()
    };
    assert_eq!(of(&s.ana), "dee@contoso.example");
    assert_eq!(of(&s.bo), Value::Null);
}

#[tokio::test]
async fn trust_takes_a_near_copy_out_of_quarantine_and_reopen_undoes_it() {
    // Issue #49.
    let g = Gate::new("trust");
    let s = sample(&g);
    // Alice has no evidence rows: a person set her kind, so a refresh keeps it.
    g.sender("alice@paypal.com", Some("person"));
    g.exec(
        "UPDATE sender SET decided_by = 'person' WHERE address = 'alice@paypal.com'",
        [],
    );
    let look = g.sender("alerts@paypa1.com", Some("hostile"));
    g.exec(
        "UPDATE sender SET basis = 'near copy of paypal.com' WHERE address = 'alerts@paypa1.com'",
        [],
    );
    // Message 5 is from the look-alike: it can be trusted.
    let m = g.get("/api/messages/m-5").await;
    assert_eq!(m["sender_kind"], "hostile");
    assert_eq!(
        m["near_copy"],
        json!({"sender": look, "of": "paypal.com", "trusted": false})
    );
    assert_eq!(g.get("/api/messages/m-1").await["near_copy"], Value::Null);

    let (code, v) = g.answer(&look, "trust").await;
    assert_eq!(code, StatusCode::OK, "{v}");
    assert_eq!(
        (v["kind"].as_str(), v["decided_by"].as_str()),
        (Some("person"), Some("person"))
    );
    assert_eq!(
        g.one::<Option<i64>>("SELECT understood_at FROM message WHERE id = 5"),
        None
    );
    let m = g.get("/api/messages/m-5").await;
    // After a reload the mail still names the trusted sender, so the
    // person can undo the trust.
    assert_eq!(
        (m["sender_kind"].as_str(), &m["near_copy"]),
        (
            Some("person"),
            &json!({"sender": look, "of": null, "trusted": true})
        )
    );

    // The trusted look-alike is no reference of the near-copy rule: the
    // real domain does not become a near copy of it.
    assert_eq!(
        g.one::<String>("SELECT kind FROM sender WHERE address = 'ana@paypal.com'"),
        "stranger"
    );
    // A sender that is not an engine near copy: no trust, no write.
    let (code, v) = g.answer(&s.ana, "trust").await;
    assert_eq!(code, StatusCode::CONFLICT);
    assert!(v["error"].as_str().unwrap().contains("near copy"), "{v}");
    assert_eq!(g.answer("p-999", "trust").await.0, StatusCode::NOT_FOUND);

    // Reopen: the engine decides again, and the mark comes back.
    let (code, v) = g.answer(&look, "reopen").await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(
        v["kind"], "hostile",
        "the answer gives the kind after reopen"
    );
    assert_eq!(
        g.one::<String>("SELECT kind FROM sender WHERE address = 'alerts@paypa1.com'"),
        "hostile"
    );
    assert_eq!(
        g.get("/api/messages/m-5").await["near_copy"],
        json!({"sender": look, "of": "paypal.com", "trusted": false})
    );

    // Rows that differ only in case: `near_copy` names the row of
    // `sender_kind` (the most cautious kind), never another row.
    g.answer(&look, "trust").await;
    let upper = g.sender("ALERTS@paypa1.com", Some("hostile"));
    g.exec(
        "UPDATE sender SET basis = 'near copy of paypal.com' WHERE address = 'ALERTS@paypa1.com'",
        [],
    );
    let m = g.get("/api/messages/m-5").await;
    assert_eq!(
        (m["sender_kind"].as_str(), &m["near_copy"]),
        (
            Some("hostile"),
            &json!({"sender": upper, "of": "paypal.com", "trusted": false})
        )
    );
    // A hostile row of another rule wins the kind and gives no trust
    // offer. The trusted row keeps its Undo, under the hostile warning
    // (issue #1043).
    g.exec(
        "UPDATE sender SET decided_by = 'person' WHERE address = 'ALERTS@paypa1.com'",
        [],
    );
    let trusted = json!({"sender": look, "of": null, "trusted": true});
    let m = g.get("/api/messages/m-5").await;
    assert_eq!(
        (m["sender_kind"].as_str(), &m["near_copy"]),
        (Some("hostile"), &trusted)
    );
    // A stranger row of another case: the same.
    g.exec(
        "UPDATE sender SET kind = 'stranger', decided_by = 'engine', basis = NULL
          WHERE address = 'ALERTS@paypa1.com'",
        [],
    );
    let m = g.get("/api/messages/m-5").await;
    assert_eq!(
        (m["sender_kind"].as_str(), &m["near_copy"]),
        (Some("stranger"), &trusted)
    );
    // Undo of that row works: the engine decides it again.
    assert_eq!(g.answer(&look, "reopen").await.0, StatusCode::OK);
    let m = g.get("/api/messages/m-5").await;
    assert_ne!(m["near_copy"]["trusted"], json!(true));
}
