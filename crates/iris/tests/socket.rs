//! The engine socket (`docs/specs/2026-11-16-engine-socket.md`): the lock,
//! the file, the protocol, and the command verbs `changeset` and `withdraw`.

use std::io::{BufRead, BufReader, Write};
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Arc;

use iris::socket::{self, code};
use iris_changeset::{Author, AuthorKind, Verb};
use iris_store::Store;
use serde_json::{json, Map, Value};

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("iris-sock-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

/// A store and its socket, served on a thread of its own.
fn engine(name: &str) -> (Arc<Store>, PathBuf) {
    engine_with(name, socket::Limits::default())
}

fn engine_with(name: &str, limits: socket::Limits) -> (Arc<Store>, PathBuf) {
    let d = dir(name);
    let store = Arc::new(Store::open(&d).unwrap());
    let bound = socket::bind(&d).unwrap();
    let s = store.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _ = rt.block_on(socket::serve_with(&bound, s, limits));
    });
    (store, d)
}

/// A draft changeset of `author` with one close action. Gives its row ID.
fn draft(store: &Store, author: &Author) -> i64 {
    let cs = iris_changeset::create(store, author, 0).unwrap();
    let mut args = Map::new();
    args.insert("reason".into(), "Done.".into());
    iris_changeset::add_action(store, cs, author, Verb::Close, &["c-1"], &args, None).unwrap();
    cs
}

fn agent() -> Author {
    Author {
        kind: AuthorKind::Agent,
        name: Some("Claude Code".into()),
        session: Some("se-1".into()),
        job: None,
    }
}

fn changesets(store: &Store) -> i64 {
    store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM changeset", [], |r| r.get(0))?))
        .unwrap()
}

/// Send raw bytes and read the answer line.
fn raw(d: &std::path::Path, bytes: &[u8]) -> Value {
    let mut s = UnixStream::connect(d.join(socket::SOCKET)).unwrap();
    s.write_all(bytes).unwrap();
    let mut line = String::new();
    BufReader::new(s).read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap_or(Value::Null)
}

#[test]
fn the_socket_has_mode_0600_and_one_engine_holds_it() {
    let d = dir("lock");
    std::fs::create_dir_all(&d).unwrap();
    let bound = socket::bind(&d).unwrap();
    let m = std::fs::symlink_metadata(d.join(socket::SOCKET)).unwrap();
    assert!(m.file_type().is_socket());
    assert_eq!(m.permissions().mode() & 0o777, 0o600);
    // A second engine fails before it touches the socket.
    let e = socket::bind(&d).err().unwrap();
    assert!(e.to_string().contains("another engine runs"), "{e}");
    assert!(d.join(socket::SOCKET).exists());
    // The drop removes the file and releases the lock.
    drop(bound);
    assert!(!d.join(socket::SOCKET).exists());
    drop(socket::bind(&d).unwrap());
}

#[test]
fn a_left_socket_goes_and_another_file_stays() {
    let d = dir("left");
    std::fs::create_dir_all(&d).unwrap();
    // A socket file that a stopped engine left.
    let left = std::os::unix::net::UnixListener::bind(d.join(socket::SOCKET)).unwrap();
    std::mem::forget(left);
    drop(socket::bind(&d).unwrap());
    // A regular file at the path is not removed (issue #266).
    std::fs::write(d.join(socket::SOCKET), b"mine").unwrap();
    let e = socket::bind(&d).err().unwrap();
    assert!(e.to_string().contains("is not a socket"), "{e}");
    assert_eq!(std::fs::read(d.join(socket::SOCKET)).unwrap(), b"mine");
}

#[test]
fn a_peer_of_another_user_is_refused() {
    assert!(socket::same_user(501, 501));
    assert!(!socket::same_user(0, 501));
}

#[test]
fn show_drafts_and_withdraw_go_through_the_socket() {
    let (store, d) = engine("verbs");
    let cs = draft(&store, &socket::local());
    let id = format!("cs-{cs}");
    let doc = socket::request(&d, "show", json!({ "changeset": id })).unwrap();
    assert_eq!(doc["id"], json!(id));
    assert_eq!(doc["actions"][0]["verb"], json!("close"));
    let list = socket::request(&d, "drafts", json!({})).unwrap();
    assert_eq!(list, json!({ "changesets": [id] }));
    let doc = socket::request(&d, "withdraw", json!({ "changeset": id })).unwrap();
    assert_eq!(doc["state"], json!("withdrawn"));
    assert_eq!(
        socket::request(&d, "drafts", json!({})).unwrap(),
        json!({ "changesets": [] })
    );

    // A changeset of another author: code 2, and it stays.
    let other = format!("cs-{}", draft(&store, &agent()));
    let e = socket::request(&d, "withdraw", json!({ "changeset": other })).unwrap_err();
    assert_eq!(e.code, code::USAGE, "{e}");
    let doc = socket::request(&d, "show", json!({ "changeset": other })).unwrap();
    assert_ne!(doc["state"], json!("withdrawn"));
    // No such changeset: code 2.
    let e = socket::request(&d, "show", json!({ "changeset": "cs-999" })).unwrap_err();
    assert_eq!(
        (e.code, e.error.as_str()),
        (code::USAGE, "no changeset cs-999")
    );
}

/// Random lines that are not a valid request give code 2 and write nothing.
#[test]
fn a_request_that_is_not_valid_writes_nothing() {
    let (store, d) = engine("bad");
    let mut fixed: Vec<Vec<u8>> = [
        "not json",
        "[]",
        "{}",
        r#"{"v":2,"verb":"drafts","args":{}}"#,
        r#"{"v":1,"verb":"drafts"}"#,
        r#"{"v":1,"verb":"drafts","args":{"x":1}}"#,
        r#"{"v":1,"verb":"nope","args":{}}"#,
        r#"{"v":1,"verb":"show","args":{"changeset":"12"}}"#,
        r#"{"v":1,"verb":"withdraw","args":{"changeset":"cs-0"}}"#,
        r#"{"v":1,"verb":"drafts","args":{},"extra":1}"#,
    ]
    .iter()
    .map(|l| format!("{l}\n").into_bytes())
    .collect();
    let mut seed = 99u64;
    for _ in 0..300 {
        let mut line: Vec<u8> = (0..(seed % 80))
            .map(|_| {
                seed = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                b"{}[]\":,v1verbargschangesetcs-0123456789 \\"[(seed >> 33) as usize % 40]
            })
            .collect();
        line.push(b'\n');
        fixed.push(line);
    }
    for line in &fixed {
        let mut a = Vec::new();
        socket::answer(&store, line, &mut a).unwrap();
        let a: Value = serde_json::from_slice(&a).unwrap();
        if a["ok"] == json!(true) {
            // Only a valid drafts request can pass, and it reads.
            assert_eq!(
                a["result"],
                json!({ "changesets": [] }),
                "{}",
                String::from_utf8_lossy(line)
            );
        } else {
            assert_eq!(
                a["code"],
                json!(code::USAGE),
                "{}",
                String::from_utf8_lossy(line)
            );
        }
    }
    assert_eq!(changesets(&store), 0);
    // Through the socket too, and a line over the limit.
    assert_eq!(raw(&d, b"not json\n")["code"], json!(code::USAGE));
    let mut long = vec![b' '; socket::MAX_LINE];
    long.push(b'\n');
    let a = raw(&d, &long);
    assert_eq!(a["code"], json!(code::USAGE));
    assert!(a["error"].as_str().unwrap().contains("limit"));
}

#[test]
fn no_engine_gives_code_4_and_a_lost_answer_gives_code_1() {
    let d = dir("none");
    std::fs::create_dir_all(&d).unwrap();
    let e = socket::request(&d, "drafts", json!({})).unwrap_err();
    assert_eq!(e.code, code::UNREACHABLE);
    // An engine that closes before the answer: the result is not known.
    let l = std::os::unix::net::UnixListener::bind(d.join(socket::SOCKET)).unwrap();
    std::thread::spawn(move || {
        for s in l.incoming().take(2) {
            let mut s = s.unwrap();
            let mut line = String::new();
            BufReader::new(&mut s).read_line(&mut line).unwrap();
        }
    });
    let e = socket::request(&d, "withdraw", json!({ "changeset": "cs-4" })).unwrap_err();
    assert_eq!(e.code, code::ERROR);
    assert!(e.error.contains("iris changeset cs-4"), "{e}");
    let e = socket::request(&d, "drafts", json!({})).unwrap_err();
    assert!(e.error.contains("iris changeset --drafts"), "{e}");
}

#[test]
fn the_command_exits_4_with_no_engine_and_shows_a_changeset_with_one() {
    let d = dir("cmd");
    std::fs::create_dir_all(&d).unwrap();
    let iris = || {
        let mut c = Command::new(env!("CARGO_BIN_EXE_iris"));
        c.arg("--data")
            .arg(&d)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        c
    };
    let out = iris().args(["withdraw", "cs-1"]).output().unwrap();
    assert_eq!(out.status.code(), Some(4));
    assert!(String::from_utf8_lossy(&out.stderr).contains("iris serve"));

    let mut serve = iris()
        .args(["serve", "--addr", "127.0.0.1:0"])
        .spawn()
        .unwrap();
    let mut line = String::new();
    let mut err = BufReader::new(serve.stderr.take().unwrap());
    while !line.contains("commands connect to") {
        line.clear();
        assert!(err.read_line(&mut line).unwrap() > 0, "the engine stopped");
    }
    let store = Store::open(&d).unwrap();
    let cs = draft(&store, &socket::local());
    let out = iris()
        .args(["changeset", &format!("cs-{cs}")])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(text.starts_with(&format!("ID: cs-{cs}\n")), "{text}");
    assert!(text.contains("1.1 close c-1 draft"), "{text}");
    assert!(text.contains(r#"args: {"reason":"Done."}"#), "{text}");
    let out = iris().args(["changeset", "--drafts"]).output().unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout), format!("cs-{cs}\n"));
    let out = iris()
        .args(["withdraw", &format!("cs-{cs}")])
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stdout).contains("State: withdrawn"));
    // A second engine on the same folder stops.
    let out = iris()
        .args(["serve", "--addr", "127.0.0.1:0"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("another engine runs"));
    serve.kill().unwrap();
    serve.wait().unwrap();
}

#[test]
fn a_slow_request_is_closed_and_the_connections_are_bounded() {
    use std::io::Read;
    use std::time::{Duration, Instant};
    let limits = socket::Limits {
        read_wait: Duration::from_millis(300),
        write_wait: Duration::from_secs(5),
        connections: 1,
    };
    let (_store, d) = engine_with("slow", limits);
    // No request line: the engine closes with no answer.
    let mut quiet = UnixStream::connect(d.join(socket::SOCKET)).unwrap();
    let start = Instant::now();
    // While the quiet connection holds the one slot, the next request waits.
    let a = raw(&d, b"{\"v\":1,\"verb\":\"drafts\",\"args\":{}}\n");
    assert!(
        start.elapsed() >= Duration::from_millis(250),
        "{:?}",
        start.elapsed()
    );
    assert_eq!(a["ok"], json!(true));
    let mut rest = Vec::new();
    quiet.read_to_end(&mut rest).unwrap();
    assert!(rest.is_empty(), "no answer to a connection with no request");
    // A partial line with no newline: at the read timeout, code 2.
    let mut part = UnixStream::connect(d.join(socket::SOCKET)).unwrap();
    part.write_all(b"{\"v\":1").unwrap();
    let mut line = String::new();
    BufReader::new(part).read_line(&mut line).unwrap();
    let a: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(a["code"], json!(code::USAGE), "{line}");
}

#[test]
fn the_command_waits_at_most_the_answer_wait_for_the_whole_answer() {
    use std::time::{Duration, Instant};
    let d = dir("drip");
    std::fs::create_dir_all(&d).unwrap();
    // An engine that sends one byte each 50 ms and never a newline.
    let l = std::os::unix::net::UnixListener::bind(d.join(socket::SOCKET)).unwrap();
    std::thread::spawn(move || {
        let mut s = l.incoming().next().unwrap().unwrap();
        let mut line = String::new();
        BufReader::new(&mut s).read_line(&mut line).unwrap();
        while s.write_all(b" ").is_ok() {
            std::thread::sleep(Duration::from_millis(50));
        }
    });
    let start = Instant::now();
    let e = socket::request_with(
        &d,
        "drafts",
        json!({}),
        Duration::from_millis(400),
        "iris changeset --drafts",
    )
    .unwrap_err();
    assert_eq!(e.code, code::ERROR);
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "{:?}",
        start.elapsed()
    );
}

/// A store with the commitment c-1 (open) and the message m-1.
fn mailbox(store: &Store) {
    let a = store.add_account("gmail", "owner@example.com").unwrap();
    let m = iris_store::NewMessage {
        provider_id: "g1".into(),
        thread_provider_id: "gt1".into(),
        ..Default::default()
    };
    store.put_messages(a, &[m]).unwrap();
    let mut doc = iris_spec::read_json(
        &iris_spec::schema_dir()
            .join("examples")
            .join("commitment-you-owe.json"),
    );
    doc["id"] = json!("c-1");
    doc["status"] = json!("open");
    store
        .write(|tx| {
            tx.execute(
                "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
                 VALUES ('commitment', ?1, 'open', ?2, 0, 0)",
                rusqlite::params![a, doc.to_string()],
            )?;
            tx.execute(
                "INSERT INTO item_source(item_id, message_id, role) VALUES (1, 1, 'origin')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
}

fn add(d: &std::path::Path, verb: &str, targets: &[&str], args: Value, cs: Option<&str>) -> Value {
    let mut r = json!({ "verb": verb, "targets": targets, "args": args });
    if let Some(cs) = cs {
        r["changeset"] = json!(cs);
    }
    socket::request(d, "add", r).unwrap()
}

#[test]
fn add_and_submit_give_the_codes_of_section_8_4() {
    let (store, d) = engine("submit");
    mailbox(&store);
    // A valid close waits for the person: code 5.
    let r = add(&d, "close", &["c-1"], json!({ "reason": "Sent it." }), None);
    assert_eq!(r["n"], json!(1));
    let cs = r["changeset"].as_str().unwrap().to_string();
    // A second action in the same draft.
    let r2 = add(&d, "read", &["m-1"], json!({}), Some(&cs));
    assert_eq!(
        (r2["changeset"].as_str(), r2["n"].as_i64()),
        (Some(cs.as_str()), Some(2))
    );
    let doc = socket::request(&d, "submit", json!({ "changeset": cs, "why": "Tidy up." })).unwrap();
    assert_eq!(iris::submit_code(&doc), code::OPEN, "{doc}");
    assert_eq!(doc["actions"][0]["decision"]["state"], json!("proposed"));

    // A target of the wrong kind is denied by the engine: code 2, not 3.
    let r = add(&d, "close", &["m-1"], json!({ "reason": "x" }), None);
    let cs2 = r["changeset"].as_str().unwrap();
    let doc = socket::request(&d, "submit", json!({ "changeset": cs2, "why": "x" })).unwrap();
    assert_eq!(
        doc["actions"][0]["decision"]["by"],
        json!("engine"),
        "{doc}"
    );
    assert_eq!(iris::submit_code(&doc), code::USAGE);

    // Hostile mail: a hard limit denies `read` by `local`, also in the mode observe.
    store
        .write(|tx| {
            Ok(tx.execute(
                "UPDATE message SET hostile = '[\"dmarc\"]' WHERE id = 1",
                [],
            )?)
        })
        .unwrap();
    let r = add(&d, "read", &["m-1"], json!({}), None);
    let cs3 = r["changeset"].as_str().unwrap();
    let doc = socket::request(&d, "submit", json!({ "changeset": cs3, "why": "x" })).unwrap();
    let decision = &doc["actions"][0]["decision"];
    assert_eq!(
        (decision["state"].as_str(), decision["by"].as_str()),
        (Some("denied"), Some("limit")),
        "{doc}"
    );
    assert_eq!(iris::submit_code(&doc), code::DENIED);
    // `archive` stays allowed on hostile mail: it waits for the person.
    let r = add(&d, "archive", &["m-1"], json!({}), None);
    let cs4 = r["changeset"].as_str().unwrap();
    let doc = socket::request(&d, "submit", json!({ "changeset": cs4, "why": "x" })).unwrap();
    assert_eq!(iris::submit_code(&doc), code::OPEN, "{doc}");
}

#[test]
fn the_submit_code_reads_only_the_new_revision() {
    let doc = json!({ "revision": 2, "actions": [
        { "revision": 1, "n": 1, "decision": { "state": "denied", "by": "rule" } },
        { "revision": 1, "n": 2, "decision": { "state": "failed", "by": "person" } },
        { "revision": 2, "n": 1, "decision": { "state": "proposed" } },
    ]});
    assert_eq!(iris::submit_code(&doc), code::OPEN);
}

#[test]
fn a_large_document_streams_and_a_stalled_reader_frees_its_slot() {
    use std::time::Duration;
    let limits = socket::Limits {
        read_wait: Duration::from_secs(5),
        write_wait: Duration::from_millis(300),
        connections: 1,
    };
    let (store, d) = engine_with("large", limits);
    let me = socket::local();
    let cs = iris_changeset::create(&store, &me, 0).unwrap();
    let why = "w".repeat(1000);
    let mut args = Map::new();
    args.insert("reason".into(), "Done.".into());
    for _ in 0..3000 {
        iris_changeset::add_action(&store, cs, &me, Verb::Close, &["c-1"], &args, Some(&why))
            .unwrap();
    }
    let id = format!("cs-{cs}");
    // About 3 MB: the whole document comes through.
    let doc = socket::request(&d, "show", json!({ "changeset": id })).unwrap();
    assert_eq!(doc["actions"].as_array().unwrap().len(), 3000);
    // A peer that sends the request and never reads holds the one slot
    // only until the write deadline.
    let mut stalled = UnixStream::connect(d.join(socket::SOCKET)).unwrap();
    stalled
        .write_all(
            format!("{{\"v\":1,\"verb\":\"show\",\"args\":{{\"changeset\":\"{id}\"}}}}\n")
                .as_bytes(),
        )
        .unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let start = std::time::Instant::now();
    let list = socket::request(&d, "drafts", json!({})).unwrap();
    assert_eq!(list, json!({ "changesets": [id] }));
    assert!(
        start.elapsed() < Duration::from_secs(3),
        "{:?}",
        start.elapsed()
    );
    drop(stalled);
    // A peer that reads slowly but without a stop also holds the slot only
    // until the deadline of the whole answer.
    let mut slow = UnixStream::connect(d.join(socket::SOCKET)).unwrap();
    slow.write_all(
        format!("{{\"v\":1,\"verb\":\"show\",\"args\":{{\"changeset\":\"{id}\"}}}}\n").as_bytes(),
    )
    .unwrap();
    std::thread::spawn(move || {
        let mut b = [0u8; 64];
        while std::io::Read::read(&mut slow, &mut b).unwrap_or(0) > 0 {
            std::thread::sleep(Duration::from_millis(5));
        }
    });
    std::thread::sleep(Duration::from_millis(100));
    let start = std::time::Instant::now();
    let list = socket::request_with(
        &d,
        "drafts",
        json!({}),
        Duration::from_secs(5),
        "iris changeset --drafts",
    )
    .unwrap();
    assert_eq!(list, json!({ "changesets": [id] }));
    assert!(
        start.elapsed() < Duration::from_secs(3),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn a_request_over_the_limit_gives_code_2_before_the_connect() {
    let d = dir("big");
    std::fs::create_dir_all(&d).unwrap();
    // No engine: code 2 comes before code 4.
    let e = socket::request(&d, "add", json!({ "why": "w".repeat(socket::MAX_LINE) })).unwrap_err();
    assert_eq!(e.code, code::USAGE, "{e}");
    // An argument cannot hold the limit (Linux keeps one at 128 KiB): a
    // draft file can.
    let file = d.join("big.json");
    let body = "w".repeat(socket::MAX_LINE);
    std::fs::write(
        &file,
        json!({ "to": ["p@x.org"], "body": body }).to_string(),
    )
    .unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_iris"))
        .arg("--data")
        .arg(&d)
        .args(["draft", "a-1", file.to_str().unwrap()])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
}

/// Each draft content that passes the checks fits in one request: a body
/// of `MAX_BODY` bytes that JSON writes 6 bytes for each byte, and the
/// most recipients, through the program and the engine.
#[test]
fn the_largest_valid_draft_goes_through_the_command() {
    let (store, d) = engine("largest");
    mailbox(&store);
    let to: Vec<String> = (0..iris_compose::MAX_RECIPIENTS)
        .map(|i| {
            format!(
                "{}{i}@{}.org",
                "p".repeat(60),
                vec!["d".repeat(60); 3].join(".")
            )
        })
        .collect();
    let body = "\u{1}".repeat(iris_compose::MAX_BODY);
    let subject = "\"".repeat(iris_compose::MAX_SUBJECT);
    let mail = json!({ "to": to, "subject": subject, "body": body });
    let content: iris_compose::Content = serde_json::from_value(mail.clone()).unwrap();
    assert!(content.check().is_ok(), "the content is not valid");
    let file = d.join("largest.json");
    std::fs::write(&file, mail.to_string()).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_iris"))
        .arg("--data")
        .arg(&d)
        .args(["draft", "a-1", file.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let args: String = store
        .read(|c| Ok(c.query_row("SELECT args FROM changeset_action", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(serde_json::from_str::<Value>(&args).unwrap(), mail);
}

/// Random answer lines: the parser never gives success without the result
/// shape of the verb, and never a code outside 1 to 5.
#[test]
fn an_answer_that_is_not_valid_is_not_a_success() {
    let mut lines: Vec<Vec<u8>> = [
        r#"{"ok":true}"#,
        r#"{"ok":true,"result":null}"#,
        r#"{"ok":true,"result":{}}"#,
        r#"{"ok":true,"result":{"changesets":[1]}}"#,
        r#"{"ok":false,"code":0,"error":"x"}"#,
        r#"{"ok":false,"code":9,"error":"x"}"#,
        r#"{"ok":false,"code":3}"#,
        r#"{"ok":"true","result":{"id":"cs-1","state":"draft","actions":[]}}"#,
        r#"{"ok":true,"result":{"session":"invalid"}}"#,
        r#"{"ok":true,"result":{"session":"se-0","reaped":true}}"#,
        r#"{"ok":true,"result":{"session":"se-01","reaped":false}}"#,
        r#"{"ok":true,"result":{"session":7,"reaped":true}}"#,
        r#"{"ok":true,"result":{"job":"j-1","goal":"g","state":"working"}}"#,
        r#"{"ok":true,"result":{"job":"j-1","session":"se-1","goal":"g","state":"working","threads":"1","messages":1,"items":1}}"#,
        r#"{"ok":true,"result":{"job":"j-1","session":"se-1","goal":"g","state":"working","threads":1,"messages":1,"items":1,"due":5}}"#,
        r#"{"ok":true,"result":{"job":"x","session":"se-1","goal":"g","state":"working","threads":1,"messages":1,"items":1}}"#,
        r#"{"ok":true,"result":{"state":"done"}}"#,
        r#"{"ok":true,"result":{"job":"j-01","state":"done"}}"#,
        r#"{"ok":true,"result":{"job":"j-1","state":"working"}}"#,
        r#"{"ok":true,"result":{"job":1,"state":"done"}}"#,
    ]
    .iter()
    .map(|l| l.as_bytes().to_vec())
    .collect();
    let mut seed = 7u64;
    for _ in 0..2000 {
        let line: Vec<u8> = (0..(seed % 90))
            .map(|_| {
                seed = seed
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                let abc = b"{}[]\":,oktruefalseresultcodeerrorid1234 nsessionreaped-jgoalstatethreadsmessagesitemsdueformtextmessageitemneeds_youquestionwaitinguntil";
                abc[(seed >> 33) as usize % abc.len()]
            })
            .collect();
        lines.push(line);
    }
    for line in &lines {
        for verb in [
            "show", "drafts", "add", "submit", "withdraw", "job.run", "job.stop", "status",
            "search", "list", "wait", "ask", "done",
        ] {
            match socket::parse_answer(verb, line) {
                None => {}
                Some(Ok(r)) if verb == "status" => assert!(
                    r["job"].as_str().and_then(iris_job::parse_job_id).is_some()
                        && r["session"]
                            .as_str()
                            .and_then(iris_job::parse_session_id)
                            .is_some()
                        && r["goal"].is_string()
                        && r["state"].is_string()
                        && ["threads", "messages", "items"]
                            .iter()
                            .all(|k| r[k].is_u64())
                        && (r["due"].is_null() || r["due"].is_string()),
                    "{verb} {}",
                    String::from_utf8_lossy(line)
                ),
                // The answer of a verb that ends the session: the job and its new state.
                Some(Ok(r)) if verb == "wait" => assert!(
                    r["job"].as_str().and_then(iris_job::parse_job_id).is_some()
                        && r["state"] == "waiting"
                        && r["until"].is_string(),
                    "{verb} {}",
                    String::from_utf8_lossy(line)
                ),
                Some(Ok(r)) if verb == "ask" => assert!(
                    r["job"].as_str().and_then(iris_job::parse_job_id).is_some()
                        && r["state"] == "needs_you"
                        && r["question"].is_string(),
                    "{verb} {}",
                    String::from_utf8_lossy(line)
                ),
                // `done`: only a canonical job ID in the state `done`.
                Some(Ok(r)) if verb == "done" => assert!(
                    r["job"].as_str().and_then(iris_job::parse_job_id).is_some()
                        && r["state"] == "done",
                    "{verb} {}",
                    String::from_utf8_lossy(line)
                ),
                Some(Ok(r)) if verb.starts_with("job.") => assert!(
                    r["session"]
                        .as_str()
                        .and_then(iris_job::parse_session_id)
                        .is_some()
                        && (verb == "job.run" || r["reaped"].is_boolean()),
                    "{verb} {}",
                    String::from_utf8_lossy(line)
                ),
                // A read of a session: a list of objects with an ID.
                Some(Ok(r)) if matches!(verb, "search" | "list") => assert!(
                    r["messages"]
                        .as_array()
                        .or(r["items"].as_array())
                        .is_some_and(|a| a.iter().all(|e| e["id"].is_string())),
                    "{verb} {}",
                    String::from_utf8_lossy(line)
                ),
                Some(Ok(r)) => assert!(
                    r.is_object()
                        && (r["changesets"].is_array()
                            || r["message"]["text"].is_string()
                            || r["item"]["form"].is_string()
                            || r["changeset"].is_string()
                            || r["id"].is_string()),
                    "{verb} {}",
                    String::from_utf8_lossy(line)
                ),
                Some(Err(f)) => assert!((1..=5).contains(&f.code)),
            }
        }
    }
    let done = br#"{"ok":true,"result":{"job":"j-3","state":"done"}}"#;
    assert!(matches!(socket::parse_answer("done", done), Some(Ok(_))));
    let ok = br#"{"ok":true,"result":{"id":"cs-1","state":"draft","actions":[]}}"#;
    assert!(matches!(socket::parse_answer("show", ok), Some(Ok(_))));
    assert!(socket::parse_answer("drafts", ok).is_none());
    let read = br#"{"ok":true,"result":{"items":[{"id":"c-1","form":"commitment"}]}}"#;
    assert!(matches!(socket::parse_answer("list", read), Some(Ok(_))));
    assert!(socket::parse_answer("search", read).is_none());
    let bad = br#"{"ok":true,"result":{"items":[{"id":"c-1"}]}}"#;
    assert!(socket::parse_answer("list", bad).is_none());
    let message = br#"{"ok":true,"result":{"message":{"id":"m-1","text":"x"}}}"#;
    assert!(matches!(socket::parse_answer("show", message), Some(Ok(_))));
    // The answers of the verbs that end a session (issue #586): the valid
    // answer succeeds, and each required field that is missing or wrong
    // makes the answer not valid.
    let wait = json!({"job": "j-4", "state": "waiting", "until": "2030-01-02T00:00:00Z"});
    let ask = json!({"job": "j-3", "state": "needs_you", "question": "Which date?"});
    let cases = [
        (
            "wait",
            wait,
            vec![
                ("job", vec![json!(7), json!("x")]),
                ("state", vec![json!("working"), json!(7)]),
                ("until", vec![json!(7), json!(null)]),
            ],
        ),
        (
            "ask",
            ask,
            vec![
                ("job", vec![json!(7), json!("x")]),
                ("state", vec![json!("waiting"), json!(7)]),
                ("question", vec![json!(7), json!(null)]),
            ],
        ),
    ];
    let line = |r: &serde_json::Value| json!({"ok": true, "result": r}).to_string().into_bytes();
    for (verb, good, fields) in cases {
        assert!(
            matches!(socket::parse_answer(verb, &line(&good)), Some(Ok(_))),
            "{verb}"
        );
        for (field, bad) in fields {
            let mut gone = good.clone();
            gone.as_object_mut().unwrap().remove(field);
            assert!(
                socket::parse_answer(verb, &line(&gone)).is_none(),
                "{verb} with no {field}"
            );
            for value in bad {
                let mut r = good.clone();
                r[field] = value.clone();
                assert!(
                    socket::parse_answer(verb, &line(&r)).is_none(),
                    "{verb} {field}={value}"
                );
            }
        }
    }
    let run = br#"{"ok":true,"result":{"session":"se-12"}}"#;
    assert!(matches!(socket::parse_answer("job.run", run), Some(Ok(_))));
    let status = br#"{"ok":true,"result":{"job":"j-1","session":"se-2","goal":"g","state":"working","due":null,"threads":1,"messages":2,"items":0}}"#;
    assert!(matches!(
        socket::parse_answer("status", status),
        Some(Ok(_))
    ));
}

/// The command adds and submits the write verbs: a new mail to an account
/// and a send of a draft keep their targets and arguments.
#[test]
fn the_command_adds_and_submits_draft_and_send() {
    let (store, d) = engine("write");
    mailbox(&store);
    store
        .write(|tx| {
            // The draft that an applied draft action made at Gmail.
            tx.execute_batch(
                "INSERT INTO changeset(author_kind, created_at) VALUES ('person', 0);
                 INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args)
                   VALUES (1, 1, 1, 'draft', '{\"accounts\":[\"a-1\"]}', '{}');
",
            )?;
            // Its raw bytes: submit flags a send from them.
            let c = iris_compose::Content {
                to: vec!["p@example.org".into()],
                cc: Vec::new(),
                subject: "Hi".into(),
                body: "Hello".into(),
            }
            .check()
            .unwrap();
            let raw = iris_compose::compose("owner@example.com", "iris.1@example.com", &c, None, 1);
            tx.execute(
                "INSERT INTO draft(account_id, provider_id, message_id_hdr, content, hash, raw,
                                   action_id, created_at, updated_at)
                 VALUES (1, 'r1', 'iris.1@example.com', '{}', ?1, ?2, 1, 0, 0)",
                rusqlite::params![iris_compose::hash(&c, None), raw],
            )?;
            Ok(())
        })
        .unwrap();
    let mail = json!({ "to": ["p@example.org"], "subject": "Hi", "body": "Hello" });
    for (verb, target, args, key) in [
        ("draft", "a-1", mail, "accounts"),
        ("send", "d-1", json!({}), "drafts"),
    ] {
        let r = add(&d, verb, &[target], args.clone(), None);
        let cs = r["changeset"].as_str().unwrap();
        let doc =
            socket::request(&d, "submit", json!({ "changeset": cs, "why": "Write." })).unwrap();
        let a = &doc["actions"][0];
        assert_eq!(a["verb"], json!(verb), "{doc}");
        assert_eq!(a["targets"][key], json!([target]), "{doc}");
        assert_eq!(a["args"], args, "{doc}");
        assert_eq!(a["decision"]["state"], json!("proposed"), "{doc}");
    }
}

/// `iris draft <target> <file>` and `iris send <draft>` add their actions
/// through the socket (specification 2, section 8.2). A file that is not
/// JSON is a usage error.
#[test]
fn the_cli_adds_draft_and_send() {
    let (store, d) = engine("cli-write");
    mailbox(&store);
    let iris = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_iris"))
            .arg("--data")
            .arg(&d)
            .args(args)
            .output()
            .unwrap()
    };
    let file = d.join("mail.json");
    let mail = json!({ "to": ["p@example.org"], "subject": "Hi", "body": "Hello" });
    std::fs::write(&file, mail.to_string()).unwrap();
    let out = iris(&["draft", "a-1", file.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let out = iris(&["send", "d-7"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let rows: Vec<(String, String, String)> = store
        .read(|c| {
            let mut st =
                c.prepare("SELECT verb, targets, args FROM changeset_action ORDER BY id")?;
            let rows = st
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
                .collect::<rusqlite::Result<_>>()?;
            Ok(rows)
        })
        .unwrap();
    let parse = |t: &str| serde_json::from_str::<Value>(t).unwrap();
    assert_eq!(rows.len(), 2, "{rows:?}");
    assert_eq!(
        (rows[0].0.as_str(), parse(&rows[0].1)),
        ("draft", json!({ "accounts": ["a-1"] }))
    );
    assert_eq!(parse(&rows[0].2), mail);
    assert_eq!(
        (rows[1].0.as_str(), parse(&rows[1].1)),
        ("send", json!({ "drafts": ["d-7"] }))
    );
    std::fs::write(&file, "to: p@example.org").unwrap();
    let out = iris(&["draft", "a-1", file.to_str().unwrap()]);
    assert_eq!(out.status.code(), Some(2));
}

/// A fake engine: it reads one request line, then sends `answer`, or
/// closes the connection with no answer if `answer` is `None`.
fn one_answer(name: &str, answer: Option<&'static str>) -> PathBuf {
    let d = dir(name);
    std::fs::create_dir_all(&d).unwrap();
    let l = std::os::unix::net::UnixListener::bind(d.join(socket::SOCKET)).unwrap();
    std::thread::spawn(move || {
        let mut s = l.incoming().next().unwrap().unwrap();
        let mut line = String::new();
        BufReader::new(&mut s).read_line(&mut line).unwrap();
        if let Some(a) = answer {
            s.write_all(a.as_bytes()).unwrap();
        }
    });
    d
}

fn job_command(d: &std::path::Path, args: &[&str]) -> Result<(), socket::Failure> {
    use clap::Parser;
    let mut all = vec!["iris", "--data", d.to_str().unwrap(), "job"];
    all.extend_from_slice(args);
    iris::run(iris::Cli::parse_from(all))
}

#[test]
fn a_lost_job_answer_names_the_job_session_command() {
    for (name, args) in [
        ("lostrun", &["run", "j-1", "--agent", "script"][..]),
        ("loststop", &["stop", "j-1"][..]),
    ] {
        let d = one_answer(name, None);
        let e = job_command(&d, args).unwrap_err();
        assert_eq!(e.code, code::ERROR, "{name}");
        assert!(
            e.error.contains("not known") && e.error.contains("iris job session j-1"),
            "{name}: {}",
            e.error
        );
        assert!(!e.error.contains("iris changeset"), "{name}: {}", e.error);
    }
}

#[test]
fn a_job_answer_of_the_engine_is_a_result() {
    let d = one_answer(
        "okrun",
        Some("{\"ok\":true,\"result\":{\"session\":\"se-1\"}}\n"),
    );
    job_command(&d, &["run", "j-1", "--agent", "script"]).unwrap();
    let d = one_answer(
        "okstop",
        Some("{\"ok\":true,\"result\":{\"session\":\"se-1\",\"reaped\":true}}\n"),
    );
    job_command(&d, &["stop", "j-1"]).unwrap();
}

#[test]
fn a_stop_that_waits_too_long_gives_the_reason() {
    let d = one_answer(
        "slowstop",
        Some("{\"ok\":true,\"result\":{\"session\":\"se-1\",\"reaped\":false,\"why\":\"the helper (process 7) still runs\"}}\n"),
    );
    let e = job_command(&d, &["stop", "j-1"]).unwrap_err();
    assert!(
        e.error.contains("the helper (process 7) still runs"),
        "{}",
        e.error
    );
}

#[test]
fn a_bad_job_id_is_a_usage_error_with_no_engine() {
    let d = dir("badjob");
    std::fs::create_dir_all(&d).unwrap();
    for args in [
        &["run", "bad", "--agent", "script"][..],
        &["stop", "j-01"][..],
        &["session", "x"][..],
    ] {
        let e = job_command(&d, args).unwrap_err();
        assert_eq!(e.code, code::USAGE, "{args:?}: {}", e.error);
        assert!(e.error.contains("not a job ID"), "{args:?}: {}", e.error);
    }
}

/// Session mode: in a folder below `.iris/`, the command sends each verb
/// to `.iris/socket` and never opens the data folder, also with `--data`.
#[test]
fn session_mode_uses_only_the_session_socket() {
    let root = dir("sessionmode");
    let work = root.join("workspace");
    let deep = work.join("drafts");
    std::fs::create_dir_all(work.join(".iris")).unwrap();
    std::fs::create_dir_all(&deep).unwrap();
    let data = root.join("data");
    let iris = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_iris"))
            .current_dir(&deep)
            .arg("--data")
            .arg(&data)
            .args(args)
            .output()
            .unwrap()
    };
    // Host mode (no `.iris/` above the folder): `wait` and `done` are usage
    // errors. `list` is a host read verb too (issue #523).
    for args in [&["wait", "--until", "2030-01-02"][..], &["done"]] {
        let out = Command::new(env!("CARGO_BIN_EXE_iris"))
            .current_dir(&root)
            .arg("--data")
            .arg(&data)
            .args(args)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{args:?}: {out:?}");
    }
    // No socket: code 4, for a socket verb and for a store verb.
    let draft = ["draft", "t-1", "missing.json"];
    for args in [
        &["status"][..],
        &["withdraw", "cs-1"],
        &["search", "x"],
        &draft,
    ] {
        let out = iris(args);
        assert_eq!(out.status.code(), Some(4), "{args:?}: {out:?}");
    }
    // A socket that answers `status`: the command prints the job.
    let listener = std::os::unix::net::UnixListener::bind(work.join(".iris/socket")).unwrap();
    let server = std::thread::spawn(move || {
        let mut verbs = Vec::new();
        for _ in 0..7 {
            let (s, _) = listener.accept().unwrap();
            let mut line = String::new();
            std::io::BufReader::new(&s).read_line(&mut line).unwrap();
            let v: serde_json::Value = serde_json::from_str(&line).unwrap();
            let verb = v["verb"].as_str().unwrap().to_string();
            let item = json!({"model": "iris/1.0", "id": "c-1", "form": "commitment",
                "sources": ["m-1"]});
            let result = match verb.as_str() {
                "search" => json!({"messages": [{"id": "m-1", "thread": "t-1",
                    "date": "2025-11-16T08:57:02Z", "from": "dana@example.com",
                    "from_name": "Dana", "subject": "Contract"}]}),
                "list" => json!({"items": [item]}),
                "show" => json!({"item": item}),
                "wait" => json!({"job": "j-4", "state": "waiting",
                    "until": "2030-01-02T00:00:00Z", "reply": "t-5"}),
                "ask" => json!({"job": "j-4", "state": "needs_you",
                    "question": "Which date works?"}),
                _ => json!({"job": "j-4", "session": "se-9", "goal": "Get it.",
                    "state": "working", "threads": 1, "messages": 2, "items": 0}),
            };
            verbs.push(format!("{verb} {}", v["args"]));
            let answer = json!({"ok": true, "result": result});
            (&s).write_all(format!("{answer}\n").as_bytes()).unwrap();
        }
        verbs
    });
    let out = iris(&["status"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("j-4 (working)"));
    // A store verb and a host verb of the socket: code 3, and nothing
    // reaches the socket.
    for args in [&["export", "out"][..], &["changeset", "--drafts"], &draft] {
        let out = iris(args);
        assert_eq!(out.status.code(), Some(3), "{args:?}: {out:?}");
    }
    let out = iris(&["status", "--json"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    // The read verbs of a session go to the socket (card R4-04f).
    let out = iris(&["search", "contract", "--limit", "5"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("m-1"),
        "{out:?}"
    );
    // `--json` prints one JSON document of the object itself.
    let out = iris(&["show", "c-1", "--json"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    let shown: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        (&shown["id"], &shown["form"]),
        (&json!("c-1"), &json!("commitment"))
    );
    // A filter of the host list is a usage error in a session, and sends
    // nothing to the socket (issue #523).
    for args in [
        &["list", "--status", "open"][..],
        &["list", "--type", "receipt"],
        &["list", "--before", "c-1"],
        &["list", "--limit", "5"],
    ] {
        assert_eq!(iris(args).status.code(), Some(2), "{args:?}");
    }
    let out = iris(&["list", "commitment"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("c-1"),
        "{out:?}"
    );
    // `wait` sends each option to the socket (R5-04).
    let out = iris(&[
        "wait",
        "--until",
        "2030-01-02",
        "--reply",
        "t-5",
        "--why",
        "Dana reads it.",
    ]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("j-4 waits until 2030-01-02T00:00:00Z"),
        "{out:?}"
    );
    // `ask` sends the question and prints the state (issue #586).
    let out = iris(&["ask", "Which date works?"]);
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout)
            .contains("j-4 asks the person and waits for the answer"),
        "{out:?}"
    );
    assert_eq!(
        server.join().unwrap(),
        [
            "status {}",
            "status {}",
            r#"search {"limit":5,"text":"contract"}"#,
            r#"show {"id":"c-1"}"#,
            r#"list {"form":"commitment"}"#,
            r#"wait {"reply":"t-5","until":"2030-01-02","why":"Dana reads it."}"#,
            r#"ask {"question":"Which date works?"}"#,
        ]
    );
    assert!(!data.exists(), "the command made the data folder");
}

/// Send one request with `socket::answer_as` and parse the answer line.
fn answer_as(store: &Store, me: &Author, request: Value) -> Value {
    let mut out = Vec::new();
    socket::answer_as(store, me, request.to_string().as_bytes(), &mut out).unwrap();
    serde_json::from_slice(&out).unwrap()
}

#[test]
fn sign_in_gives_a_new_address_each_time() {
    let (_, d) = engine("sign-in");
    let auth = socket::AUTH
        .get_or_init(|| Arc::new(iris_api::Auth::new("127.0.0.1:4800".parse().unwrap()).unwrap()));
    let url = |_| {
        socket::request(&d, "sign-in", json!({})).unwrap()["url"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let (a, b) = (url(0), url(1));
    assert!(a.starts_with("http://127.0.0.1:4800/#code="), "{a}");
    assert_ne!(a, b);
    // The start code stays as it was.
    assert!(auth.sign_in_url().is_some());
    let e = socket::request(&d, "sign-in", json!({ "x": 1 })).unwrap_err();
    assert_eq!(e.code, code::USAGE);
    // `iris open --print` prints a new address. With no engine: code 4.
    let open = |data: &std::path::Path| {
        Command::new(env!("CARGO_BIN_EXE_iris"))
            .arg("--data")
            .arg(data)
            .args(["open", "--print"])
            .output()
            .unwrap()
    };
    let out = open(&d);
    assert_eq!(out.status.code(), Some(0));
    let printed = String::from_utf8(out.stdout).unwrap();
    assert!(
        printed.starts_with("http://127.0.0.1:4800/#code="),
        "{printed}"
    );
    assert!(![a, b].contains(&printed.trim().to_string()));
    let none = dir("open-none");
    std::fs::create_dir_all(&none).unwrap();
    assert_eq!(
        open(&none).status.code(),
        Some(i32::from(code::UNREACHABLE))
    );
}

#[test]
fn a_session_cannot_sign_in_or_read_the_notices_of_the_app() {
    let d = dir("app-session");
    let store = Store::open(&d).unwrap();
    let mut me = agent();
    me.job = Some("j-1".into());
    for verb in ["sign-in", "app.notices"] {
        let r = answer_as(&store, &me, json!({ "v": 1, "verb": verb, "args": {} }));
        assert_eq!(r["code"], json!(code::DENIED), "{verb}: {r}");
        assert!(r.get("result").is_none(), "{verb}: {r}");
    }
}

#[test]
fn app_notices_give_only_the_new_notices_oldest_first() {
    let d = dir("app-notices");
    let store = Store::open(&d).unwrap();
    let ask = |args: Value| {
        answer_as(
            &store,
            &socket::local(),
            json!({ "v": 1, "verb": "app.notices", "args": args }),
        )
    };
    // No notice: nothing, and the last ID is 0.
    let r = ask(json!({}));
    assert_eq!(
        (
            &r["result"]["notices"],
            &r["result"]["last"],
            &r["result"]["open"]
        ),
        (&json!([]), &json!(0), &json!(0))
    );
    store
        .write(|tx| {
            tx.execute_batch(
                "INSERT INTO notice(message_id, rule, rule_name, policy_version, title, text, created_at)
                 VALUES (1, 7, 'Clinic', 1, 'Dr Lee', 'Results', 0),
                        (2, 7, 'Clinic', 1, 'Dr Lee', 'Moved', 0),
                        (3, 7, 'Clinic', 1, 'Dr Lee', 'Done', 0);
                 UPDATE notice SET done_at = 1 WHERE id = 3;",
            )?;
            Ok(())
        })
        .unwrap();
    // The start of the engine stays the same in one process.
    let engine = r["result"]["engine"].clone();
    assert!(engine.as_u64().is_some_and(|e| e > 0), "{r}");
    // A new app gets no old notice, only the last ID (a done notice too).
    let r = ask(json!({}));
    assert_eq!(r["result"]["start"], json!(true), "{r}");
    assert_eq!(
        (
            &r["result"]["notices"],
            &r["result"]["last"],
            &r["result"]["open"]
        ),
        (&json!([]), &json!(3), &json!(2))
    );
    // The first call started the app after notice 3: the engine still shows
    // notices 1 to 3 itself (a notice of the first call is not lost), and
    // leaves a later notice to the app.
    let now = iris::engine::now_ms();
    assert!((1..=3).all(|id| !iris::notify::app_shows_now(id, now)));
    assert!(iris::notify::app_shows_now(4, now));
    let r = ask(json!({ "after": 0 }));
    let ids: Vec<&str> = r["result"]["notices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, ["n-1", "n-2"]);
    assert_eq!(r["result"]["notices"][0]["text"], json!("Results"));
    assert_eq!(r["result"]["last"], json!(2));
    let r = ask(json!({ "after": 2 }));
    assert_eq!(r["result"]["start"], json!(false), "{r}");
    assert_eq!(
        (
            &r["result"]["notices"],
            &r["result"]["last"],
            &r["result"]["open"]
        ),
        (&json!([]), &json!(2), &json!(2))
    );
    // A long list comes in pages of APP_NOTICES_MAX, oldest first.
    store
        .write(|tx| {
            for m in 10..35 {
                tx.execute(
                    "INSERT INTO notice(message_id, rule, rule_name, policy_version, title, text, created_at)
                     VALUES (?1, 7, 'Clinic', 1, 'Dr Lee', 'More', 0)",
                    [m],
                )?;
            }
            Ok(())
        })
        .unwrap();
    let r = ask(json!({ "after": 3 }));
    let page = r["result"]["notices"].as_array().unwrap();
    assert_eq!(page.len(), socket::APP_NOTICES_MAX as usize);
    assert_eq!(page[0]["id"], json!("n-4"));
    assert_eq!(r["result"]["last"], json!(23));
    assert_eq!(r["result"]["open"], json!(27));
    assert_eq!(r["result"]["engine"], engine);
    let r = ask(json!({ "after": 23 }));
    assert_eq!(r["result"]["notices"].as_array().unwrap().len(), 5);
    assert_eq!(r["result"]["last"], json!(28));
    for bad in [
        json!({ "after": -1 }),
        json!({ "after": null }),
        json!({ "after": "2" }),
        json!({ "after": 1.5 }),
        json!({ "since": 1 }),
    ] {
        let r = ask(bad.clone());
        assert_eq!(r["code"], json!(code::USAGE), "{bad}: {r}");
    }
}

#[test]
fn iris_open_keeps_the_code_out_of_process_arguments() {
    use std::os::unix::fs::PermissionsExt;
    let d = dir("open-page");
    std::fs::create_dir_all(&d).unwrap();
    let url = "http://127.0.0.1:4800/#code=0123abcd";
    // An old page is removed. A link with the name of a page is never
    // written through.
    let old = d.join("open-1-1.html");
    std::fs::write(&old, b"old").unwrap();
    let ago = std::time::SystemTime::now() - iris_api::CODE_LIFE * 2;
    std::fs::File::options()
        .write(true)
        .open(&old)
        .unwrap()
        .set_modified(ago)
        .unwrap();
    let target = d.join("target");
    std::fs::write(&target, b"mine").unwrap();
    std::os::unix::fs::symlink(&target, d.join("open-2-2.html")).unwrap();
    let page = iris::open_page(&d, url).unwrap();
    assert_eq!(std::fs::read(&target).unwrap(), b"mine");
    assert!(
        std::fs::symlink_metadata(&old).is_err(),
        "the old page is gone"
    );
    let m = std::fs::symlink_metadata(&page).unwrap();
    assert!(m.file_type().is_file());
    assert_eq!(m.permissions().mode() & 0o777, 0o600);
    // Two opens before the browser reads either page: two pages, each with
    // its own code. A new page stays.
    let other = "http://127.0.0.1:4800/#code=4567ef01";
    let second = iris::open_page(&d, other).unwrap();
    assert_ne!(page, second);
    assert!(std::fs::read_to_string(&second).unwrap().contains(other));
    assert!(page.exists());
    let html = std::fs::read_to_string(&page).unwrap();
    assert!(html.contains(&format!("content=\"0;url={url}\"")), "{html}");
    for os in ["macos", "linux"] {
        let c = iris::opener(os, &page).unwrap();
        let args: Vec<_> = c.get_args().collect();
        assert_eq!(args, [page.as_os_str()], "{os}");
        assert!(!c.get_program().to_string_lossy().contains("code"));
    }
    assert!(iris::opener("windows", &page).is_none());
    for bad in [
        "http://127.0.0.1:4800/#code=\"><script>",
        "file:///etc/passwd",
        "javascript:x",
    ] {
        assert!(iris::open_page(&d, bad).is_err(), "{bad}");
    }
}

/// `iris skip`, `mute` and `dismiss` add the state verb with the default
/// reason of the web client, or the given reason (issue #441).
#[test]
fn skip_mute_and_dismiss_add_the_state_verb() {
    use clap::Parser;
    let cases = [
        (vec!["skip", "f-3"], "skip", "Skipped."),
        (vec!["mute", "v-2"], "mute", "Muted."),
        (vec!["dismiss", "s-1"], "dismiss", "Dismissed."),
        (
            vec!["mute", "v-2", "--reason", "Done here."],
            "mute",
            "Done here.",
        ),
    ];
    for (argv, verb, reason) in cases {
        let cli =
            iris::Cli::try_parse_from(std::iter::once("iris").chain(argv.iter().copied())).unwrap();
        let request = iris::add_request(&cli.command).unwrap().unwrap();
        assert_eq!(
            request,
            json!({ "verb": verb, "targets": [argv[1]], "args": { "reason": reason } }),
            "{argv:?}"
        );
    }
    let cli = iris::Cli::try_parse_from(["iris", "skip", "f-3", "--changeset", "cs-4"]).unwrap();
    let request = iris::add_request(&cli.command).unwrap().unwrap();
    assert_eq!(request["changeset"], json!("cs-4"));
}

/// `--until` of `iris wait`: a day gives its start in UTC, and an RFC 3339
/// time with any offset gives its instant (issue #556). Property: each
/// instant of whole seconds, written with any offset of -14:00 to +14:00,
/// reads back to the same millisecond, and the RFC 3339 text of the answer
/// reads back to the same end. A fraction of a second other than zero is an
/// error, because each output writes seconds. Other text is an error, never
/// a panic.
#[test]
fn the_end_of_a_wait_is_a_day_or_an_rfc3339_time() {
    use chrono::{FixedOffset, TimeZone};
    assert_eq!(socket::until_ms("2026-11-18"), Ok(1_794_960_000_000));
    assert_eq!(
        socket::until_ms("2026-11-18T00:00:00Z"),
        Ok(1_794_960_000_000)
    );
    assert_eq!(
        socket::until_ms("2026-11-18T09:30:00-08:00"),
        Ok(1_794_960_000_000 + (17 * 60 + 30) * 60_000)
    );
    let mut x = 0x2545_F491_4F6C_DD1Du64;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    for _ in 0..2_000 {
        // 1970 to about 2100, in milliseconds.
        let ms = (next() % 4_102_444_800) as i64 * 1000;
        let offset = (next() % (28 * 60 + 1)) as i32 - 14 * 60;
        let zone = FixedOffset::east_opt(offset * 60).unwrap();
        let t = zone.timestamp_millis_opt(ms).unwrap();
        let text = t.to_rfc3339_opts(chrono::SecondsFormat::Millis, offset == 0);
        assert_eq!(socket::until_ms(&text), Ok(ms), "{text}");
        // The answer of `wait` shows the same end that the job stores.
        assert_eq!(socket::until_ms(&iris_api::rfc3339(ms)), Ok(ms), "{text}");
        // Any fraction other than zero: an error, not a shown end that differs.
        let frac = 1 + (next() % 999) as i64;
        let t = zone.timestamp_millis_opt(ms + frac).unwrap();
        let text = t.to_rfc3339_opts(chrono::SecondsFormat::Millis, offset == 0);
        assert!(socket::until_ms(&text).is_err(), "{text}");
    }
    for bad in [
        "",
        "2026-11",
        "2026-13-01",
        "2026-11-18 09:30",
        "2026-11-18T09:30",
        "2026-11-18T25:00:00Z",
        "tomorrow",
        "2026-11-18T09:30:00+15:00x",
        "\u{0662}026-11-18",
        "2026-11-18T09:30:00",
        "2026-11-18T09:30:00.750Z",
    ] {
        assert!(socket::until_ms(bad).is_err(), "{bad:?}");
    }
    for _ in 0..2_000 {
        let s: String = (0..(next() % 30))
            .map(|_| b"0123456789-:TZ+. x"[(next() % 18) as usize] as char)
            .collect();
        let _ = socket::until_ms(&s);
    }
}
