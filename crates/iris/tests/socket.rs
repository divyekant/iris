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
    // Through the socket too, and a line over 64 KiB.
    assert_eq!(raw(&d, b"not json\n")["code"], json!(code::USAGE));
    let mut long = vec![b' '; socket::MAX_LINE];
    long.push(b'\n');
    let a = raw(&d, &long);
    assert_eq!(a["code"], json!(code::USAGE));
    assert!(a["error"].as_str().unwrap().contains("64 KiB"));
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
}
