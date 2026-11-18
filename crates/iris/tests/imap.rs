//! The IMAP account of the engine (docs/specs/2026-11-17-imap-provider.md,
//! section 8): `iris account add-imap`, the sealed password, the provider of
//! each account and the sync against the fake IMAP server. Each run of the
//! program sets `IRIS_TEST_IMAP_PLAIN`, so the program connects to the fake
//! server on 127.0.0.1 with no TLS.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use iris_fakeimap::{Caps, FakeImap, PASSWORD, USER};
use iris_store::Store;
use iris_synth::Mailbox;

const OWNER: &str = "owner@example.org";

fn data_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("iris-imap-engine-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// Run the program with `input` on standard input. Gives (success, stdout,
/// stderr).
fn run(dir: &Path, args: &[&str], input: &str) -> (bool, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_iris"))
        .args(args)
        .arg("--data")
        .arg(dir)
        .env("IRIS_TEST_IMAP_PLAIN", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let (stdout, stderr) = (
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    );
    // The password is never in the output of the program.
    for t in [&stdout, &stderr] {
        assert!(!t.contains(PASSWORD), "{t}");
    }
    (out.status.success(), stdout, stderr)
}

fn add(dir: &Path, fake: &FakeImap, address: &str, password: &str) -> (bool, String, String) {
    let port = fake.addr().port().to_string();
    run(
        dir,
        &[
            "account",
            "add-imap",
            address,
            "--host",
            "127.0.0.1",
            "--port",
            &port,
            "--user",
            USER,
        ],
        &format!("{password}\n"),
    )
}

/// (provider, token, state, imap_port) of the account with `address`.
type Row = (String, Option<Vec<u8>>, Option<String>, Option<i64>);

fn row(dir: &Path, address: &str) -> Option<Row> {
    use rusqlite::OptionalExtension;
    let store = Store::open(dir).unwrap();
    store
        .read(|c| {
            Ok(c.query_row(
                "SELECT provider, token, state, imap_port FROM account WHERE address = ?1",
                [address],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?)
        })
        .unwrap()
}

fn mailbox(fake: &FakeImap) {
    let mail = Mailbox::new(31, 6);
    for i in 0..mail.len() {
        fake.add("INBOX", mail.raw(i), &[]);
    }
}

#[test]
fn an_imap_account_syncs_with_zero_differences() {
    let fake = FakeImap::start(Caps::ALL);
    mailbox(&fake);
    let dir = data_dir("sync");
    let (ok, out, err) = add(&dir, &fake, OWNER, PASSWORD);
    assert!(ok, "{out}{err}");
    let (provider, token, state, _) = row(&dir, OWNER).unwrap();
    assert_eq!((provider.as_str(), state.as_deref()), ("imap", Some("ok")));
    // The password is sealed: the column does not hold its bytes.
    let token = token.unwrap();
    assert!(!token
        .windows(PASSWORD.len())
        .any(|w| w == PASSWORD.as_bytes()));

    let (ok, out, err) = run(&dir, &["mirror", "sync"], "");
    assert!(ok, "{out}{err}");
    let (ok, out, err) = run(&dir, &["mirror", "verify"], "");
    assert!(ok && out.contains(" 0 differences"), "{out}{err}");
    let store = Store::open(&dir).unwrap();
    let n: i64 = store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM message", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(n, 6);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_refused_password_adds_nothing() {
    let fake = FakeImap::start(Caps::ALL);
    let dir = data_dir("refused");
    let (ok, _, err) = add(&dir, &fake, OWNER, "not the password");
    assert!(
        !ok && err.contains("refused the user or the password"),
        "{err}"
    );
    assert_eq!(row(&dir, OWNER), None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_address_keeps_its_provider_and_its_mailbox() {
    let fake = FakeImap::start(Caps::ALL);
    let dir = data_dir("address");
    // A Gmail account with the address: refused, and no change.
    let store = Store::open(&dir).unwrap();
    store.add_account("gmail", OWNER).unwrap();
    drop(store);
    let (ok, _, err) = add(&dir, &fake, OWNER, PASSWORD);
    assert!(!ok && err.contains("another provider"), "{err}");
    assert_eq!(
        row(&dir, OWNER).unwrap(),
        ("gmail".into(), None, None, None)
    );

    // An IMAP account: a new password for the same server and user only.
    let other = "other@example.org";
    assert!(add(&dir, &fake, other, PASSWORD).0);
    let first = row(&dir, other).unwrap();
    let second = FakeImap::start(Caps::ALL);
    let (ok, _, err) = add(&dir, &second, other, PASSWORD);
    assert!(!ok && err.contains("another IMAP server"), "{err}");
    assert_eq!(row(&dir, other).unwrap(), first);
    assert!(add(&dir, &fake, other, PASSWORD).0);
    let again = row(&dir, other).unwrap();
    assert_ne!(again.1, first.1, "a new seal of the password");
    assert_eq!(again.3, first.3);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_refused_stored_password_needs_a_new_sign_in() {
    let fake = FakeImap::start(Caps::ALL);
    mailbox(&fake);
    let dir = data_dir("needs");
    assert!(add(&dir, &fake, OWNER, PASSWORD).0);
    let store = Store::open(&dir).unwrap();
    let bad = store.seal(b"an old password").unwrap();
    store
        .write(|tx| {
            tx.execute("UPDATE account SET token = ?1", [&bad])?;
            Ok(())
        })
        .unwrap();
    drop(store);
    let (ok, _, err) = run(&dir, &["mirror", "sync"], "");
    assert!(!ok && err.contains("add-imap"), "{err}");
    assert_eq!(
        row(&dir, OWNER).unwrap().2.as_deref(),
        Some("needs_sign_in")
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_plain_test_mode_refuses_a_host_that_is_not_local() {
    let dir = data_dir("plain");
    let (ok, _, err) = run(
        &dir,
        &["account", "add-imap", OWNER, "--host", "imap.example.com"],
        &format!("{PASSWORD}\n"),
    );
    assert!(!ok && err.contains("127.0.0.1"), "{err}");
    assert_eq!(row(&dir, OWNER), None);
    let _ = std::fs::remove_dir_all(&dir);
}
