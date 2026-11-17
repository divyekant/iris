//! The engine with a fake Gmail and a fake token endpoint: no network, no
//! real mailbox and no real token.

mod common;

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::Duration;

use common::*;
use iris::engine::{self, Engine, Worker, STATE_NEEDS_SIGN_IN, STATE_OK};
use iris_fakegmail::FakeGmail;
use iris_store::Store;
use iris_synth::Mailbox;

struct Setup {
    store: Arc<Store>,
    fake: FakeGmail,
    tokens: TokenServer,
    account: i64,
}

impl Setup {
    fn worker(&self, account: i64) -> Worker {
        Worker::new(
            self.store.clone(),
            gmail(self.fake.base_url()),
            account,
            Some(self.tokens.uri.clone()),
        )
    }
}

fn setup(name: &str, count: usize) -> Setup {
    let fake = FakeGmail::start(Mailbox::new(7, count)).unwrap();
    let tokens = token_server();
    let store = Arc::new(Store::open(&data_dir(name)).unwrap());
    let account = add_account(&store, &gmail(fake.base_url()));
    Setup {
        store,
        fake,
        tokens,
        account,
    }
}

fn token_of(store: &Store, account: i64) -> Vec<u8> {
    store
        .read(|c| {
            Ok(
                c.query_row("SELECT token FROM account WHERE id = ?1", [account], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap()
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn the_refresh_token_and_the_client_are_stored_sealed() {
    let s = setup("sealed", 1);
    let sealed = token_of(&s.store, s.account);
    assert!(
        !contains(&sealed, REFRESH.as_bytes()),
        "plain refresh token in iris.db"
    );
    assert!(
        !contains(&sealed, CLIENT_SECRET.as_bytes()),
        "plain client secret in iris.db"
    );
    let plain = String::from_utf8(s.store.unseal(&sealed).unwrap()).unwrap();
    assert!(plain.contains(REFRESH) && plain.contains(CLIENT_SECRET));
    assert_state(&s.store, s.account, STATE_OK);
    // No file outside iris.db holds the client.
    for f in std::fs::read_dir(s.store.dir()).unwrap() {
        let path = f.unwrap().path();
        if path.is_file() {
            let bytes = std::fs::read(&path).unwrap();
            assert!(!contains(&bytes, CLIENT_SECRET.as_bytes()), "{path:?}");
        }
    }
}

#[test]
fn a_sync_gets_a_token_from_the_stored_sign_in() {
    let s = setup("first", 30);
    let mut w = s.worker(s.account);
    assert_eq!(w.sync().unwrap().fetched, 30);
    assert_eq!(
        count(
            &s.store,
            "SELECT count(*) FROM account WHERE mirror_ready_at IS NOT NULL"
        ),
        1
    );
    let calls = s.tokens.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 1);
    assert!(calls[0].contains("grant_type=refresh_token"));
    assert!(calls[0].contains(REFRESH));
    // The second sync uses the token in memory.
    w.sync().unwrap();
    assert_eq!(s.tokens.calls.lock().unwrap().len(), 1);
}

#[test]
fn a_401_gets_a_new_token_one_time() {
    let s = setup("unauthorized", 5);
    let mut w = s.worker(s.account);
    w.sync().unwrap();
    s.fake.fail_next(&[401]);
    w.sync().unwrap();
    assert_eq!(s.tokens.calls.lock().unwrap().len(), 2);
}

#[test]
fn a_401_after_a_new_token_needs_sign_in() {
    let s = setup("unauthorized-twice", 5);
    let mut w = s.worker(s.account);
    w.sync().unwrap();
    s.fake.fail_next(&[401, 401]);
    let r = w.sync();
    assert!(matches!(r, Err(engine::Error::NeedsSignIn)), "{r:?}");
    assert_state(&s.store, s.account, STATE_NEEDS_SIGN_IN);
    assert!(engine::sync_accounts(&s.store).unwrap().is_empty());
}

/// A proxy in front of the fake Gmail. For each of the first `times` tokens
/// (one 401 for each token, counted by the token server calls), the first
/// raw fetch waits `wait`, then the fake answers HTTP 401: a token that ran
/// out while the call worked.
fn expiring_proxy(fake: &FakeGmail, tokens: &TokenServer, times: usize, wait: Duration) -> String {
    use std::net::{Shutdown, TcpListener, TcpStream};
    use std::sync::atomic::{AtomicUsize, Ordering};
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let target = fake.addr();
    let fake = fake.clone();
    let fired = Arc::new(AtomicUsize::new(0));
    let calls = tokens.calls.clone();
    std::thread::spawn(move || {
        for client in listener.incoming() {
            let Ok(client) = client else { continue };
            let server = TcpStream::connect(target).unwrap();
            let (mut s_read, mut c_write) =
                (server.try_clone().unwrap(), client.try_clone().unwrap());
            std::thread::spawn(move || {
                let _ = std::io::copy(&mut s_read, &mut c_write);
                let _ = c_write.shutdown(Shutdown::Both);
            });
            let (fake, fired, calls) = (fake.clone(), fired.clone(), calls.clone());
            std::thread::spawn(move || {
                let (mut c_read, mut s_write) = (client, server);
                let mut buf = [0u8; 8192];
                loop {
                    let n = match std::io::Read::read(&mut c_read, &mut buf) {
                        Ok(0) | Err(_) => break,
                        Ok(n) => n,
                    };
                    let raw = buf[..n].windows(10).any(|w| w == b"format=raw");
                    let token = calls.lock().unwrap().len();
                    if raw
                        && token <= times
                        && fired
                            .try_update(Ordering::SeqCst, Ordering::SeqCst, |f| {
                                (f < token).then_some(token)
                            })
                            .is_ok()
                    {
                        std::thread::sleep(wait);
                        fake.fail_next(&[401]);
                    }
                    if s_write.write_all(&buf[..n]).is_err() {
                        break;
                    }
                }
                let _ = s_write.shutdown(Shutdown::Both);
            });
        }
    });
    format!("http://{addr}/gmail/v1")
}

#[test]
fn tokens_that_run_out_during_one_sync_need_no_sign_in() {
    let s = setup("ran-out", 40);
    // Each token lives 61 s: 1 s more than the margin, so it runs out 1 s
    // after it came (a real token lives 1 hour).
    let short = tokens_answer().1.replace("3600", "61");
    s.tokens
        .answers
        .lock()
        .unwrap()
        .extend((0..4).map(|_| (200, short.clone())));
    // Two 401s, each after the token of the call ran out. The old rule
    // counted them as two refusals.
    let mut w = Worker::new(
        s.store.clone(),
        gmail(expiring_proxy(
            &s.fake,
            &s.tokens,
            2,
            Duration::from_millis(1_200),
        )),
        s.account,
        Some(s.tokens.uri.clone()),
    );
    let r = w.sync();
    assert!(r.is_ok(), "{r:?}");
    // Each new call goes on from the last one: all 40 are in the mirror.
    assert_eq!(
        count(
            &s.store,
            "SELECT count(*) FROM message WHERE fetched_at IS NOT NULL"
        ),
        40
    );
    assert_state(&s.store, s.account, STATE_OK);
    assert_eq!(s.tokens.calls.lock().unwrap().len(), 3);
}

#[test]
fn a_refused_refresh_token_needs_sign_in() {
    let s = setup("revoked", 5);
    s.tokens
        .answers
        .lock()
        .unwrap()
        .push_back((400, r#"{"error":"invalid_grant"}"#.into()));
    let r = s.worker(s.account).sync();
    assert!(matches!(r, Err(engine::Error::NeedsSignIn)), "{r:?}");
    assert_state(&s.store, s.account, STATE_NEEDS_SIGN_IN);
    assert!(engine::sync_accounts(&s.store).unwrap().is_empty());
}

#[test]
fn a_token_endpoint_error_keeps_the_account() {
    let s = setup("token-down", 5);
    s.tokens
        .answers
        .lock()
        .unwrap()
        .push_back((500, r#"{"error":"internal_failure"}"#.into()));
    let mut w = s.worker(s.account);
    assert!(w.sync().is_err());
    assert_state(&s.store, s.account, STATE_OK);
    // The next sync tries again.
    w.sync().unwrap();
}

#[test]
fn a_failed_old_sign_in_does_not_undo_a_new_sign_in() {
    let s = setup("stale", 1);
    let old = token_of(&s.store, s.account);
    // `iris account add` again, while a refresh with the old sign-in runs.
    add_account(&s.store, &gmail(s.fake.base_url()));
    assert!(!engine::mark_needs_sign_in(&s.store, s.account, &old).unwrap());
    assert_state(&s.store, s.account, STATE_OK);
    let new = token_of(&s.store, s.account);
    assert!(engine::mark_needs_sign_in(&s.store, s.account, &new).unwrap());
    assert_state(&s.store, s.account, STATE_NEEDS_SIGN_IN);
}

#[test]
fn a_new_sign_in_replaces_the_token_in_memory() {
    let s = setup("new-sign-in", 3);
    let mut w = s.worker(s.account);
    w.sync().unwrap();
    // `iris account add` with another client, while the first token is valid.
    let other = client_json().replace("fake-id", "other-id");
    sign_in(&s.store, &gmail(s.fake.base_url()), &other, "refresh-new");
    w.sync().unwrap();
    let calls = s.tokens.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert!(
        calls[1].contains("client_id=other-id") && calls[1].contains("refresh-new"),
        "{calls:?}"
    );
}

#[test]
fn each_account_refreshes_with_its_own_client() {
    let s = setup("two-clients", 1);
    rename_account(&s.store, s.account, "first@example.com");
    let other = client_json().replace("fake-id", "other-id");
    let second = sign_in(
        &s.store,
        &gmail(s.fake.base_url()),
        &other,
        "refresh-of-other",
    );
    let _ = s.worker(s.account).sync(); // WrongMailbox: the fake has one address.
    s.worker(second).sync().unwrap();
    let calls = s.tokens.calls.lock().unwrap().clone();
    assert!(
        calls[0].contains("client_id=fake-id") && calls[0].contains(REFRESH),
        "{calls:?}"
    );
    assert!(
        calls[1].contains("client_id=other-id") && calls[1].contains("refresh-of-other"),
        "{calls:?}"
    );
}

#[test]
fn a_stalled_account_does_not_stop_another_account() {
    let s = setup("stalled", 20);
    rename_account(&s.store, s.account, "stalled@example.com");
    *s.tokens.stall.lock().unwrap() = Some(REFRESH.to_string());
    let second = sign_in(
        &s.store,
        &gmail(s.fake.base_url()),
        &client_json(),
        "refresh-two",
    );
    let mut e = Engine::new(
        s.store.clone(),
        config(s.fake.base_url()),
        Some(s.tokens.uri.clone()),
    );
    assert_eq!(e.tick().unwrap(), vec![s.account, second]);
    wait_for(
        &s.store,
        &format!(
            "SELECT count(*) FROM account WHERE id = {second} AND mirror_ready_at IS NOT NULL"
        ),
        Duration::from_secs(30),
    );
    // A thread runs for each account: the next tick starts none.
    assert!(e.tick().unwrap().is_empty());
}

#[test]
fn an_account_added_while_the_engine_runs_starts_to_sync() {
    let fake = FakeGmail::start(Mailbox::new(7, 3)).unwrap();
    let tokens = token_server();
    let store = Arc::new(Store::open(&data_dir("late-add")).unwrap());
    let mut e = Engine::new(
        store.clone(),
        config(fake.base_url()),
        Some(tokens.uri.clone()),
    );
    assert!(e.tick().unwrap().is_empty(), "no account yet");
    let account = add_account(&store, &gmail(fake.base_url()));
    assert_eq!(e.tick().unwrap(), vec![account]);
    wait_for(
        &store,
        "SELECT count(*) FROM account WHERE mirror_ready_at IS NOT NULL",
        Duration::from_secs(30),
    );
}

fn assert_state(store: &Store, account: i64, state: &str) {
    let s: String = store
        .read(|c| {
            Ok(
                c.query_row("SELECT state FROM account WHERE id = ?1", [account], |r| {
                    r.get(0)
                })?,
            )
        })
        .unwrap();
    assert_eq!(s, state);
}

#[test]
fn account_add_signs_in_with_a_pasted_address() {
    let fake = FakeGmail::start(Mailbox::new(7, 1)).unwrap();
    let tokens = token_server();
    let dir = data_dir("add");
    let client = dir.with_extension("client.json");
    std::fs::write(&client, client_json()).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_iris"))
        .args(["account", "add", "--paste", "--data"])
        .arg(&dir)
        .arg(&client)
        .env("IRIS_TEST_GMAIL_URL", fake.base_url())
        .env("IRIS_TEST_TOKEN_URI", &tokens.uri)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stderr = BufReader::new(child.stderr.take().unwrap());
    let url = loop {
        let mut line = String::new();
        assert!(
            stderr.read_line(&mut line).unwrap() > 0,
            "no sign-in address"
        );
        if line.starts_with("https://accounts.google.com/") {
            break line;
        }
    };
    let state = url
        .split(['?', '&'])
        .find_map(|p| p.strip_prefix("state="))
        .unwrap()
        .trim();
    writeln!(
        child.stdin.take().unwrap(),
        "http://127.0.0.1:1/oauth/callback?state={state}&code=fake-code&scope={}",
        iris_auth::SCOPE
    )
    .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("added owner@example.com"));
    assert!(tokens.calls.lock().unwrap()[0].contains("grant_type=authorization_code"));

    let store = Arc::new(Store::open(&dir).unwrap());
    assert_eq!(engine::sync_accounts(&store).unwrap(), vec![1]);
    Worker::new(store, gmail(fake.base_url()), 1, Some(tokens.uri.clone()))
        .sync()
        .unwrap();
}

#[test]
fn test_overrides_refuse_an_address_that_is_not_local() {
    let dir = data_dir("override");
    let client = dir.with_extension("client.json");
    std::fs::write(&client, client_json()).unwrap();
    for (var, value, error) in [
        (
            "IRIS_TEST_TOKEN_URI",
            "https://attacker.example/token",
            "iris: the OAuth client file is not usable: a test endpoint must be http://127.0.0.1:<port>/",
        ),
        (
            "IRIS_TEST_GMAIL_URL",
            "http://127.0.0.1:80@attacker.example/gmail/v1",
            "iris: IRIS_TEST_GMAIL_URL must be http://127.0.0.1:<port>",
        ),
    ] {
        let out = Command::new(env!("CARGO_BIN_EXE_iris"))
            .args(["account", "add", "--paste", "--data"])
            .arg(&dir)
            .arg(&client)
            .env(var, value)
            .stdin(Stdio::null())
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(!out.status.success(), "{var}");
        // The command stops before the sign-in starts: no token can exist.
        assert_eq!(stderr.trim(), error, "{var}");
    }
}

#[test]
fn backup_copies_the_data_folder_with_the_sign_in() {
    let s = setup("backup", 10);
    s.worker(s.account).sync().unwrap();
    let dest = s.store.dir().with_extension("copy");
    let _ = std::fs::remove_dir_all(&dest);
    let out = Command::new(env!("CARGO_BIN_EXE_iris"))
        .args(["backup", "--data"])
        .arg(s.store.dir())
        .arg(&dest)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("10 content files"));
    let copy = Arc::new(Store::open(&dest).unwrap());
    assert_eq!(count(&copy, "SELECT count(*) FROM message"), 10);
    // The copy has the key and the sign-in: it syncs.
    Worker::new(
        copy,
        gmail(s.fake.base_url()),
        s.account,
        Some(s.tokens.uri.clone()),
    )
    .sync()
    .unwrap();
}

#[test]
fn a_tick_opens_a_deferred_commitment_on_its_date() {
    let store = Arc::new(Store::open(&data_dir("deferred")).unwrap());
    // No token: the tick starts no sync worker.
    let account = store.add_account("gmail", "owner@example.com").unwrap();
    store
        .write(|tx| {
            tx.execute(
                "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
                 VALUES ('commitment', ?1, 'deferred',
                   '{\"status\":\"deferred\",\"deferred_until\":\"2000-01-01\"}', 0, 0)",
                [account],
            )?;
            Ok(())
        })
        .unwrap();
    let mut e = Engine::new(store.clone(), config("http://127.0.0.1:9".into()), None);
    assert!(e.tick().unwrap().is_empty());
    let status: String = store
        .read(|c| Ok(c.query_row("SELECT status FROM item", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(status, "open");
}

#[test]
fn understanding_runs_apart_from_the_sync_and_after_a_held_lock() {
    let s = setup("engine-understand", 5);
    // Another run holds the lock: the sync must not wait for it.
    let held = iris_understand::batch::lock(&s.store).unwrap();
    let mut e = Engine::new(
        s.store.clone(),
        config(s.fake.base_url()),
        Some(s.tokens.uri.clone()),
    );
    assert_eq!(e.tick().unwrap(), vec![s.account]);
    wait_for(
        &s.store,
        "SELECT count(*) FROM account WHERE mirror_ready_at IS NOT NULL",
        Duration::from_secs(30),
    );
    assert_eq!(count(&s.store, "SELECT count(*) FROM body"), 0);
    // The run is owed, and runs when the lock is free. The body rows (the
    // words of search) come from understanding.
    drop(held);
    wait_for(
        &s.store,
        "SELECT count(*) FROM header",
        Duration::from_secs(30),
    );
    wait_for(
        &s.store,
        "SELECT count(*) FROM understand_run WHERE ended_at IS NOT NULL",
        Duration::from_secs(30),
    );
}

#[test]
fn the_engine_writes_the_attention_of_each_item() {
    let s = setup("engine-attention", 5);
    s.worker(s.account).sync().unwrap();
    s.store
        .write(|tx| {
            tx.execute(
                "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
                 VALUES ('feed', ?1, 'new', '{}', 0, 0)",
                [s.account],
            )?;
            tx.execute(
                "INSERT INTO item_source(item_id, message_id, role)
                 SELECT last_insert_rowid(), min(id), 'origin' FROM message",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    let mut e = Engine::new(
        s.store.clone(),
        config(s.fake.base_url()),
        Some(s.tokens.uri.clone()),
    );
    e.tick().unwrap();
    wait_for(
        &s.store,
        "SELECT count(*) FROM item WHERE json_extract(doc, '$.attention.state') = 'shown'",
        Duration::from_secs(30),
    );
}

#[test]
fn mail_that_waits_for_a_model_is_read_again_only_after_a_change() {
    let s = setup("engine-idle", 5);
    s.worker(s.account).sync().unwrap();
    assert!(matches!(
        iris::cost::after_sync(&s.store, engine::now_ms()).unwrap(),
        iris::cost::AfterSync::Ran { .. }
    ));
    let rows = count(&s.store, "SELECT count(*) FROM understand_run");
    let waiting = count(
        &s.store,
        "SELECT max(id) FROM message WHERE waits_model = 1",
    );
    assert!(waiting > 0, "with no model, mail waits");
    // No change: each turn reads nothing and leaves no row.
    for _ in 0..3 {
        s.worker(s.account).sync().unwrap();
        assert_eq!(
            iris::cost::after_sync(&s.store, engine::now_ms()).unwrap(),
            iris::cost::AfterSync::Idle
        );
    }
    assert_eq!(count(&s.store, "SELECT count(*) FROM understand_run"), rows);
    // A SPAM label, by any write path, clears the mark: the free steps
    // read the message with no model. A run that the lock blocks is owed.
    s.store
        .write(|tx| {
            tx.execute(
                "INSERT OR IGNORE INTO label(account_id, provider_id, name, kind)
                 VALUES (?1, 'SPAM', 'SPAM', 'system')",
                [s.account],
            )?;
            tx.execute(
                "INSERT OR IGNORE INTO message_label(message_id, label_id)
                 SELECT ?1, id FROM label WHERE account_id = ?2 AND provider_id = 'SPAM'",
                [waiting, s.account],
            )?;
            Ok(())
        })
        .unwrap();
    let held = iris_understand::batch::lock(&s.store).unwrap();
    assert_eq!(
        iris::cost::after_sync(&s.store, engine::now_ms()).unwrap(),
        iris::cost::AfterSync::Busy
    );
    drop(held);
    assert!(matches!(
        iris::cost::after_sync(&s.store, engine::now_ms()).unwrap(),
        iris::cost::AfterSync::Ran { .. }
    ));
    assert_eq!(
        count(
            &s.store,
            &format!(
                "SELECT count(*) FROM message WHERE id = {waiting} AND understood_at IS NOT NULL"
            )
        ),
        1,
        "the SPAM mail is read with no model"
    );
    assert_eq!(
        iris::cost::after_sync(&s.store, engine::now_ms()).unwrap(),
        iris::cost::AfterSync::Idle
    );
}

/// Each tick stores the triage proposal of each account (R2-05), and a
/// second tick with no change adds no revision.
#[test]
fn the_engine_stores_the_triage_proposal() {
    let s = setup("engine-triage", 5);
    s.worker(s.account).sync().unwrap();
    s.store
        .write(|tx| {
            tx.execute(
                "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
                 VALUES ('feed', ?1, 'new', '{}', 0, 0)",
                [s.account],
            )?;
            tx.execute(
                "INSERT INTO item_source(item_id, message_id, role)
                 SELECT last_insert_rowid(), min(ml.message_id), 'origin'
                 FROM message_label ml JOIN label l ON l.id = ml.label_id
                 WHERE l.provider_id = 'INBOX'",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    let mut e = Engine::new(
        s.store.clone(),
        config(s.fake.base_url()),
        Some(s.tokens.uri.clone()),
    );
    e.tick().unwrap();
    let triage = "SELECT count(*) FROM changeset_action a JOIN changeset c ON c.id = a.changeset_id
                  WHERE c.author_kind = 'triage' AND a.state = 'proposed'";
    assert_eq!(count(&s.store, triage), 1);
    e.tick().unwrap();
    assert_eq!(count(&s.store, triage), 1);
}

/// The tick applies a state action that the person approved (R2-10).
#[test]
fn a_tick_applies_an_approved_close() {
    let store = Arc::new(Store::open(&data_dir("close")).unwrap());
    // No token: the tick starts no sync worker.
    let account = store.add_account("gmail", "owner@example.com").unwrap();
    store
        .write(|tx| {
            tx.execute(
                "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
                 VALUES ('commitment', ?1, 'open', '{\"id\":\"c-1\",\"status\":\"open\"}', 0, 0)",
                [account],
            )?;
            Ok(())
        })
        .unwrap();
    let person = iris_changeset::Author {
        kind: iris_changeset::AuthorKind::Person,
        name: None,
        session: None,
        job: None,
    };
    let args = serde_json::json!({ "reason": "Done." });
    let cs = iris_changeset::create(&store, &person, 1).unwrap();
    let verb = iris_changeset::Verb::Close;
    iris_changeset::add_action(
        &store,
        cs,
        &person,
        verb,
        &["c-1"],
        args.as_object().unwrap(),
        None,
    )
    .unwrap();
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    iris_changeset::submit(&store, cs, &person, "Done.", t).unwrap();
    let hash: String = store
        .read(|c| Ok(c.query_row("SELECT hash FROM changeset_action", [], |r| r.get(0))?))
        .unwrap();
    let action = iris_changeset::ActionId {
        changeset: cs,
        revision: 1,
        n: 1,
    };
    iris_changeset::approve(&store, action, &hash, t).unwrap();
    let mut e = Engine::new(store.clone(), config("http://127.0.0.1:9".into()), None);
    assert!(e.tick().unwrap().is_empty());
    let status: String = store
        .read(|c| Ok(c.query_row("SELECT status FROM item", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(status, "done");
}

#[test]
fn a_tick_finds_the_mail_of_a_watch() {
    let store = Arc::new(Store::open(&data_dir("watch")).unwrap());
    // No token: the tick starts no sync worker.
    let account = store.add_account("gmail", "owner@example.com").unwrap();
    let w = store.add_watch("visa", iris::engine::now_ms()).unwrap();
    let m = store
        .put_messages(
            account,
            &[iris_store::NewMessage {
                provider_id: "a".into(),
                thread_provider_id: "t".into(),
                internal_date: Some(iris::engine::now_ms()),
                body: Some(iris_store::Body {
                    subject: "Visa".into(),
                    sender: "embassy@example.com".into(),
                    text: "Your visa is ready.".into(),
                }),
                ..Default::default()
            }],
        )
        .unwrap()[0];
    let mut e = Engine::new(store.clone(), config("http://127.0.0.1:9".into()), None);
    e.tick().unwrap();
    assert_eq!(store.watch(w).unwrap().unwrap().message, Some(m));
}
