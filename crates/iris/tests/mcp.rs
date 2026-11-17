//! `iris mcp` (card R5-11, specification 2, section 8.5): a scripted MCP
//! client on standard input and output, against an engine on a mailbox
//! that the fake Gmail server synced.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Arc;

use clap::Parser;
use common::*;
use iris::engine::Worker;
use iris::socket;
use iris_fakegmail::FakeGmail;
use iris_store::Store;
use iris_synth::Mailbox;
use serde_json::{json, Map, Value};

/// A synced mailbox, and its engine socket on a thread of its own.
fn engine(name: &str) -> (Arc<Store>, FakeGmail) {
    let fake = FakeGmail::start(Mailbox::new(3, 20)).unwrap();
    let tokens = token_server();
    let store = Arc::new(Store::open(&data_dir(name)).unwrap());
    add_account(&store, &gmail(fake.base_url()));
    Worker::new(store.clone(), gmail(fake.base_url()), 1, Some(tokens.uri))
        .sync()
        .unwrap();
    let bound = socket::bind(store.dir()).unwrap();
    let s = store.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let _ = rt.block_on(socket::serve(&bound, s));
    });
    (store, fake)
}

/// A scripted MCP client of `iris mcp`.
struct Client {
    child: Child,
    input: Option<ChildStdin>,
    output: BufReader<ChildStdout>,
    id: u64,
}

impl Client {
    fn start(dir: &Path) -> Client {
        let mut child = Command::new(env!("CARGO_BIN_EXE_iris"))
            .arg("--data")
            .arg(dir)
            .arg("mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut c = Client {
            input: child.stdin.take(),
            output: BufReader::new(child.stdout.take().unwrap()),
            child,
            id: 0,
        };
        let init = c.ask(
            "initialize",
            json!({ "protocolVersion": "2025-06-18", "capabilities": {},
                    "clientInfo": { "name": "script", "version": "1" } }),
        );
        assert_eq!(
            init["result"]["serverInfo"]["name"],
            json!("iris"),
            "{init}"
        );
        assert!(
            init["result"]["capabilities"]["tools"].is_object(),
            "{init}"
        );
        c.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }));
        c
    }

    fn send(&mut self, v: Value) {
        let input = self.input.as_mut().unwrap();
        writeln!(input, "{v}").unwrap();
        input.flush().unwrap();
    }

    /// Send one request and read its answer. Each line is one message.
    fn ask(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        let id = self.id;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }));
        loop {
            let mut line = String::new();
            assert!(
                self.output.read_line(&mut line).unwrap() > 0,
                "the server stopped"
            );
            let v: Value = serde_json::from_str(&line).unwrap();
            if v["id"] == json!(id) {
                return v;
            }
        }
    }

    /// Call a tool. Gives what the command prints, and the exit code from
    /// the last block of an error result (0 for a result that is not an
    /// error).
    fn run(&mut self, name: &str, args: Value) -> (String, u8) {
        let r = self.ask("tools/call", json!({ "name": name, "arguments": args }));
        let result = &r["result"];
        let mut blocks: Vec<String> = result["content"]
            .as_array()
            .unwrap_or_else(|| panic!("{r}"))
            .iter()
            .map(|b| b["text"].as_str().unwrap().to_string())
            .collect();
        if result["isError"] != json!(true) {
            return (blocks.concat(), 0);
        }
        let error = blocks.pop().unwrap();
        let code = error
            .rsplit_once("(exit code ")
            .and_then(|(_, c)| c.trim_end_matches(')').parse().ok())
            .unwrap_or_else(|| panic!("no exit code in {error}"));
        (blocks.concat(), code)
    }

    /// The error text of a call that fails with `code`.
    fn error(&mut self, name: &str, args: Value, code: u8) -> String {
        let r = self.ask("tools/call", json!({ "name": name, "arguments": args }));
        let text = r["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default();
        assert_eq!(r["result"]["isError"], json!(true), "{r}");
        assert!(text.ends_with(&format!("(exit code {code})")), "{text}");
        text.to_string()
    }

    fn ok(&mut self, name: &str, args: Value) -> Value {
        let (text, code) = self.run(name, args);
        assert_eq!(code, 0, "{name}: {text}");
        serde_json::from_str(&text).unwrap()
    }

    /// Close the input: the server stops with code 0.
    fn stop(mut self) {
        drop(self.input.take());
        assert_eq!(self.child.wait().unwrap().code(), Some(0));
    }
}

#[test]
fn a_client_reads_the_mail_and_submits_a_changeset() {
    let (store, _fake) = engine("mcp");
    let mut c = Client::start(store.dir());

    let list = c.ask("tools/list", json!({}));
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    for verb in [
        "status", "search", "show", "archive", "draft", "send", "submit", "withdraw",
    ] {
        assert!(names.contains(&verb), "{verb} not in {names:?}");
    }
    // No verb of the person or of the host: approvals stay in the web client.
    for verb in ["serve", "account", "sending", "approve", "model"] {
        assert!(!names.contains(&verb), "{verb}");
    }

    // The read verbs give the JSON of `--json`.
    let status = c.ok("status", json!({}));
    assert_eq!(
        status,
        serde_json::to_value(iris_api::status(&store).unwrap()).unwrap()
    );
    let (lines, code) = c.run("search", json!({ "text": "label:INBOX", "limit": 2 }));
    assert_eq!(code, 0, "{lines}");
    assert_eq!(lines.lines().count(), 2, "{lines}");
    let first: Value = serde_json::from_str(lines.lines().next().unwrap()).unwrap();
    let m = first["id"].as_str().unwrap().to_string();
    assert_eq!(c.ok("show", json!({ "id": m }))["id"], json!(m));

    // Two actions in one draft changeset, then the submit: the policy
    // decides as for the command, and the archive waits for the person.
    let r = c.ok("archive", json!({ "ids": [m], "why": "Old." }));
    let cs = r["changeset"].as_str().unwrap().to_string();
    assert_eq!(r["n"], json!(1));
    let r = c.ok(
        "label",
        json!({ "ids": [m], "label": "Done", "changeset": cs }),
    );
    assert_eq!(
        (r["changeset"].as_str(), r["n"].as_i64()),
        (Some(cs.as_str()), Some(2))
    );
    let (text, code) = c.run("submit", json!({ "changeset": cs, "why": "Tidy up." }));
    assert_eq!(code, socket::code::OPEN, "{text}");
    let doc: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(doc["id"], json!(cs), "{doc}");
    assert_eq!(doc["author"]["kind"], json!("local"), "{doc}");
    assert_eq!(doc["actions"][0]["verb"], json!("archive"));
    assert_eq!(
        doc["actions"][0]["decision"]["state"],
        json!("proposed"),
        "{doc}"
    );
    assert_eq!(
        c.ok("changeset", json!({ "changeset": cs }))["id"],
        json!(cs)
    );

    // Hostile mail: the hard limit denies `read`, as for the command.
    let (_, row) = iris_store::parse_id(&m).unwrap();
    store
        .write(|tx| {
            Ok(tx.execute(
                "UPDATE message SET hostile = '[\"dmarc\"]' WHERE id = ?1",
                [row],
            )?)
        })
        .unwrap();
    let r = c.ok("read", json!({ "ids": [m] }));
    let (text, code) = c.run(
        "submit",
        json!({ "changeset": r["changeset"], "why": "Seen." }),
    );
    assert_eq!(code, socket::code::DENIED, "{text}");
    let doc: Value = serde_json::from_str(&text).unwrap();
    let d = &doc["actions"][0]["decision"];
    assert_eq!(
        (d["state"].as_str(), d["by"].as_str()),
        (Some("denied"), Some("limit")),
        "{doc}"
    );

    // A wrong argument is a usage error, and a tool that does not exist is
    // an error of the protocol. Neither writes a changeset.
    let before = count(&store, "SELECT count(*) FROM changeset");
    c.error("archive", json!({ "ids": [] }), socket::code::USAGE);
    c.error("show", json!({ "id": "m-999999" }), socket::code::ERROR);
    let r = c.ask("tools/call", json!({ "name": "approve", "arguments": {} }));
    assert!(r["error"].is_object(), "{r}");
    assert_eq!(count(&store, "SELECT count(*) FROM changeset"), before);
    c.stop();
}

/// Each tool gives what its verb prints with `--json`, and the exit code of
/// the verb.
#[test]
fn each_tool_gives_the_output_and_the_code_of_its_verb() {
    let (store, _fake) = engine("mcp-same");
    let mut c = Client::start(store.dir());
    let cli = |args: &[&str]| {
        let out = Command::new(env!("CARGO_BIN_EXE_iris"))
            .arg("--data")
            .arg(store.dir())
            .args(args)
            .output()
            .unwrap();
        (
            String::from_utf8(out.stdout).unwrap(),
            out.status.code().unwrap() as u8,
        )
    };
    let (text, _) = c.run("search", json!({ "text": "label:INBOX", "limit": 1 }));
    let m: Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    let m = m["id"].as_str().unwrap().to_string();
    // No draft: no line on both paths.
    assert_eq!(c.run("drafts", json!({})), (String::new(), 0));
    assert_eq!(cli(&["changeset", "--drafts", "--json"]), (String::new(), 0));
    let draft = c.ok("archive", json!({ "ids": [m] }))["changeset"]
        .as_str()
        .unwrap()
        .to_string();
    // A changeset that a hard limit denies: the code is 3.
    let (_, row) = iris_store::parse_id(&m).unwrap();
    store
        .write(|tx| {
            Ok(tx.execute(
                "UPDATE message SET hostile = '[\"dmarc\"]' WHERE id = ?1",
                [row],
            )?)
        })
        .unwrap();
    let denied = c.ok("read", json!({ "ids": [m] }))["changeset"]
        .as_str()
        .unwrap()
        .to_string();
    let (_, code) = c.run("submit", json!({ "changeset": denied, "why": "x" }));
    assert_eq!(code, socket::code::DENIED);
    let cases: Vec<(Vec<&str>, &str, Value)> = vec![
        (vec!["status", "--json"], "status", json!({})),
        (
            vec!["search", "label:INBOX", "--limit", "3", "--json"],
            "search",
            json!({ "text": "label:INBOX", "limit": 3 }),
        ),
        (vec!["show", &m, "--json"], "show", json!({ "id": m })),
        (
            vec!["show", "m-999999", "--json"],
            "show",
            json!({ "id": "m-999999" }),
        ),
        (
            vec!["changeset", &draft, "--json"],
            "changeset",
            json!({ "changeset": draft }),
        ),
        (
            vec!["changeset", &denied, "--json"],
            "changeset",
            json!({ "changeset": denied }),
        ),
        (
            vec!["changeset", "cs-999", "--json"],
            "changeset",
            json!({ "changeset": "cs-999" }),
        ),
        (vec!["changeset", "--drafts", "--json"], "drafts", json!({})),
        (
            vec!["submit", "cs-999", "--why", "x", "--json"],
            "submit",
            json!({ "changeset": "cs-999", "why": "x" }),
        ),
    ];
    for (argv, tool, args) in cases {
        let mcp = c.run(tool, args);
        assert_eq!(mcp, cli(&argv), "{argv:?}");
    }
    // A withdraw and a revise change the changeset: each gives the document
    // that `changeset --json` shows after it. A second call fails the same
    // way on both paths.
    for (tool, cs) in [("withdraw", &draft), ("revise", &denied)] {
        let mcp = c.run(tool, json!({ "changeset": cs }));
        assert_eq!(mcp, cli(&["changeset", cs, "--json"]), "{tool}");
        let again = c.run(tool, json!({ "changeset": cs }));
        assert_ne!(again.1, 0, "{tool}");
        assert_eq!(again, cli(&[tool, cs, "--json"]), "{tool}");
    }
    // After the withdraw and the revise: only the revised changeset is a draft.
    let drafts = c.run("drafts", json!({}));
    assert_eq!(drafts, (format!("\"{denied}\"\n"), 0));
    assert_eq!(drafts, cli(&["changeset", "--drafts", "--json"]));
    c.stop();
}

#[test]
fn with_no_engine_a_write_tool_gives_code_4() {
    let dir = data_dir("mcp-no-engine");
    std::fs::create_dir_all(&dir).unwrap();
    let mut c = Client::start(&dir);
    let text = c.error(
        "archive",
        json!({ "ids": ["m-1"] }),
        socket::code::UNREACHABLE,
    );
    assert!(text.contains("iris serve"), "{text}");
    c.stop();
}

/// Each tool that adds an action sends the same `add` request as its verb
/// of the command.
#[test]
fn each_add_tool_sends_the_request_of_its_verb() {
    let file = std::env::temp_dir().join(format!("iris-mcp-mail-{}.json", std::process::id()));
    let mail = json!({ "to": ["p@example.org"], "subject": "Hi", "body": "Hello" });
    std::fs::write(&file, mail.to_string()).unwrap();
    let f = file.to_str().unwrap();
    let cases: Vec<(Vec<&str>, &str, Value)> = vec![
        (
            vec!["archive", "t-1", "m-2"],
            "archive",
            json!({ "ids": ["t-1", "m-2"] }),
        ),
        (
            vec!["inbox", "t-1", "--changeset", "cs-4"],
            "inbox",
            json!({ "ids": ["t-1"], "changeset": "cs-4" }),
        ),
        (
            vec!["read", "m-1", "--why", "Seen."],
            "read",
            json!({ "ids": ["m-1"], "why": "Seen." }),
        ),
        (vec!["unread", "m-1"], "unread", json!({ "ids": ["m-1"] })),
        (vec!["trash", "m-1"], "trash", json!({ "ids": ["m-1"] })),
        (
            vec!["label", "m-1", "--label", "A"],
            "label",
            json!({ "ids": ["m-1"], "label": "A" }),
        ),
        (
            vec!["unlabel", "m-1", "--label", "A"],
            "unlabel",
            json!({ "ids": ["m-1"], "label": "A" }),
        ),
        (vec!["close", "c-1"], "close", json!({ "id": "c-1" })),
        (
            vec!["close", "c-1", "--reason", "Sent."],
            "close",
            json!({ "id": "c-1", "reason": "Sent." }),
        ),
        (vec!["decline", "c-1"], "decline", json!({ "id": "c-1" })),
        (
            vec!["defer", "c-1", "--date", "2026-12-01"],
            "defer",
            json!({ "id": "c-1", "date": "2026-12-01" }),
        ),
        (
            vec![
                "correct",
                "c-1",
                "--field",
                "due.date",
                "--value",
                "\"2026-12-01\"",
            ],
            "correct",
            json!({ "id": "c-1", "field": "due.date", "value": "2026-12-01" }),
        ),
        (
            vec!["correct", "c-1", "--field", "n", "--value", "3"],
            "correct",
            json!({ "id": "c-1", "field": "n", "value": 3 }),
        ),
        (
            vec!["draft", "a-1", f],
            "draft",
            json!({ "target": "a-1", "draft": mail }),
        ),
        (vec!["send", "d-4"], "send", json!({ "id": "d-4" })),
    ];
    for (argv, tool, args) in cases {
        let cli =
            iris::Cli::try_parse_from(std::iter::once("iris").chain(argv.iter().copied())).unwrap();
        let command = iris::add_request(&cli.command).unwrap().unwrap();
        let args: Map<String, Value> = args.as_object().cloned().unwrap();
        let mcp = iris_mcp::call(tool, &args).unwrap().unwrap();
        assert_eq!(
            mcp,
            iris_mcp::Call::Socket {
                verb: "add",
                args: command
            },
            "{argv:?}"
        );
    }
}
