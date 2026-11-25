//! The moves of an IMAP account: `archive`, `inbox`, `trash` and `untrash`
//! through `move_plan` and `relocate`, with the rebind of the store row
//! (docs/specs/2026-11-25-imap-moves.md, sections 3 to 5, and the failure
//! list), at the fake IMAP server. No network.

use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;

use iris_apply::{apply, Outcome, Token};
use iris_fakeimap::{Caps, FakeImap, Moving, PASSWORD, USER};
use iris_imap::Imap;
use iris_store::Store;
use iris_synth::Mailbox;
use rusqlite::params;

const OWNER: &str = "owner@example.org";
const PERSON: &str = "decided_by = 'person', decided_at = 2";

struct Box {
    fake: FakeImap,
    imap: Imap<TcpStream>,
    store: Store,
    account: i64,
    dir: PathBuf,
}

impl Drop for Box {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// A server with `\Archive`, `\Trash` and a user folder `Work`.
fn server(caps: Caps) -> FakeImap {
    let fake = FakeImap::start(caps);
    fake.create("Archive", Some("\\Archive"));
    fake.create("Trash", Some("\\Trash"));
    fake.create("Work", None);
    fake
}

/// The fake after `fill`, then a first sync into a new store.
fn setup(name: &str, fake: FakeImap) -> Box {
    let addr = fake.addr();
    let imap = Imap::new(OWNER, USER, move || {
        let s = TcpStream::connect(addr)?;
        s.set_read_timeout(Some(Duration::from_secs(10)))?;
        Ok(s)
    });
    let dir = std::env::temp_dir().join(format!("iris-apply-i-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let account = store.add_account("imap", OWNER).unwrap();
    iris_sync::sync(&store, &imap, account, PASSWORD).unwrap();
    store
        .write(|tx| {
            tx.execute(
                "INSERT INTO policy_version(text, sha256, confirmed_at)
                 VALUES ('version = 0\nmode = \"apply\"\n', 'h', 0)",
                [],
            )?;
            tx.execute("UPDATE message SET hostile = NULL", [])?;
            tx.execute("DELETE FROM sender WHERE kind = 'hostile'", [])?;
            Ok(())
        })
        .unwrap();
    Box {
        fake,
        imap,
        store,
        account,
        dir,
    }
}

fn add(fake: &FakeImap, folder: &str, n: u64) -> u32 {
    fake.add(folder, Mailbox::new(n, 1).raw(0), &["\\Seen"])
}

impl Box {
    fn row(&self, folder: &str, uid: u32) -> i64 {
        let id = format!("{}:{uid}:{folder}", self.fake.snapshot(folder).0);
        self.store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT id FROM message WHERE provider_id = ?1",
                    [&id],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
    }

    fn one<T: rusqlite::types::FromSql>(&self, sql: &str, p: impl rusqlite::Params) -> T {
        self.store
            .read(|c| Ok(c.query_row(sql, p, |r| r.get(0))?))
            .unwrap()
    }

    fn provider_id(&self, message: i64) -> String {
        self.one("SELECT provider_id FROM message WHERE id = ?1", [message])
    }

    /// Message rows: all, and gone.
    fn rows(&self) -> (i64, i64) {
        (
            self.one("SELECT count(*) FROM message", []),
            self.one("SELECT count(*) FROM message WHERE gone_at IS NOT NULL", []),
        )
    }

    fn approved(&self, verb: &str, messages: &[i64]) -> i64 {
        let ids: Vec<String> = messages.iter().map(|m| format!("\"m-{m}\"")).collect();
        let targets = format!("{{\"messages\":[{}]}}", ids.join(","));
        self.store
            .write(|tx| {
                tx.execute(
                    "INSERT INTO changeset(author_kind, created_at) VALUES ('agent', 0)",
                    [],
                )?;
                let cs = tx.last_insert_rowid();
                tx.execute(
                    "INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args)
                     VALUES (?1, 1, 1, ?2, ?3, '{}')",
                    params![cs, verb, targets],
                )?;
                let a = tx.last_insert_rowid();
                for m in messages {
                    tx.execute(
                        "INSERT INTO action_message(action_id, message_id) VALUES (?1, ?2)",
                        params![a, m],
                    )?;
                }
                let (_, h) = iris_changeset::hash(tx, a).unwrap();
                tx.execute(
                    "UPDATE changeset_action SET hash = ?2 WHERE id = ?1",
                    params![a, h],
                )?;
                tx.execute(
                    "UPDATE changeset SET why = 'w', submitted_at = ?2 WHERE id = ?1",
                    params![cs, now()],
                )?;
                tx.execute(
                    &format!(
                        "UPDATE changeset_action SET state = 'approved', {PERSON} WHERE id = ?1"
                    ),
                    [a],
                )?;
                Ok(a)
            })
            .unwrap()
    }

    fn try_apply(&self, action: i64) -> iris_apply::Result<Outcome> {
        let mut renew = || Some(PASSWORD.to_string());
        let mut token = Token {
            token: PASSWORD.into(),
            renew: &mut renew,
            refused: false,
        };
        apply(&self.store, &self.imap, &mut token, self.account, action)
    }

    fn apply(&self, action: i64) -> Outcome {
        self.try_apply(action).unwrap()
    }

    fn undo(&self, action: i64) -> Vec<Outcome> {
        let entry: i64 = self.one("SELECT id FROM ledger WHERE action_id = ?1", [action]);
        let actions = iris_apply::undo(&self.store, entry, now()).unwrap();
        actions.iter().map(|&a| self.apply(a)).collect()
    }

    fn sync(&self) {
        iris_sync::sync(&self.store, &self.imap, self.account, PASSWORD).unwrap();
    }

    /// `iris_sync::verify`: zero differences between the mirror and the server.
    fn verify(&self, what: &str) {
        let r = iris_sync::verify(&self.store, &self.imap, self.account, PASSWORD).unwrap();
        assert_eq!(r.total, 0, "{what}: {:?}", r.differences);
    }

    fn moves_sent(&self) -> usize {
        self.fake
            .commands()
            .iter()
            .filter(|c| c.contains("UID MOVE"))
            .count()
    }

    fn listed(&self, action: i64) -> Vec<Option<String>> {
        self.store
            .read(|c| {
                let mut st = c.prepare(
                    "SELECT lm.listed FROM ledger_message lm JOIN ledger l ON l.id = lm.ledger_id
                     WHERE l.action_id = ?1 ORDER BY lm.message_id",
                )?;
                let rows = st.query_map([action], |r| r.get(0))?;
                Ok(rows.collect::<rusqlite::Result<_>>()?)
            })
            .unwrap()
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

fn changed(o: Outcome) -> u64 {
    match o {
        Outcome::Applied(c) => c.changed,
        other => panic!("{other:?}"),
    }
}

fn failed(o: Outcome) -> String {
    match o {
        Outcome::Failed { counts, error } => {
            assert_eq!(counts.changed, 0, "{error}");
            error
        }
        other => panic!("{other:?}"),
    }
}

/// `archive`, sync, undo (`inbox`), sync; then `trash`, sync, undo
/// (`untrash`), sync: the same store rows, no gone row, and zero
/// differences, with QRESYNC and with CONDSTORE only.
#[test]
fn move_and_undo_keep_the_row() {
    for (name, qresync) in [("q", true), ("c", false)] {
        let fake = server(Caps {
            qresync,
            ..Caps::ALL
        });
        let (ua, ub) = (add(&fake, "INBOX", 1), add(&fake, "INBOX", 2));
        let b = setup(&format!("keep-{name}"), fake);
        let ms = [b.row("INBOX", ua), b.row("INBOX", ub)];
        let rows = b.rows();
        assert_eq!(rows.1, 0);
        for (verb, folder) in [("archive", ":Archive"), ("trash", ":Trash")] {
            let a = b.approved(verb, &ms);
            assert_eq!(changed(b.apply(a)), 2, "{name} {verb}");
            for &m in &ms {
                assert!(b.provider_id(m).ends_with(folder), "{}", b.provider_id(m));
            }
            b.sync();
            assert_eq!(b.rows(), rows, "{name} {verb}: no new row, no gone row");
            b.verify(&format!("{name} {verb}"));
            let outcomes = b.undo(a);
            assert!(
                outcomes.iter().all(|o| matches!(o, Outcome::Applied(_))),
                "{name} {verb}: {outcomes:?}"
            );
            for &m in &ms {
                assert!(b.provider_id(m).ends_with(":INBOX"), "{}", b.provider_id(m));
            }
            b.sync();
            assert_eq!(b.rows(), rows, "{name} {verb} undo");
            b.verify(&format!("{name} {verb} undo"));
        }
        assert!(b.moves_sent() >= 4);
    }
}

/// A message with no `Message-ID`, `References` or `In-Reply-To`: its
/// thread ID is its message ID, and the thread follows the move.
#[test]
fn move_of_a_message_with_no_thread_headers() {
    let fake = server(Caps::ALL);
    let raw =
        b"From: Ana <ana@example.org>\r\nTo: owner@example.org\r\nSubject: plain\r\n\r\nhello\r\n";
    let uid = fake.add("INBOX", raw.to_vec(), &["\\Seen"]);
    let b = setup("nothread", fake);
    let m = b.row("INBOX", uid);
    let thread = |b: &Box| -> String {
        b.one(
            "SELECT t.provider_id FROM message m JOIN thread t ON t.id = m.thread_id
             WHERE m.id = ?1",
            [m],
        )
    };
    assert_eq!(thread(&b), b.provider_id(m));
    let a = b.approved("archive", &[m]);
    assert_eq!(changed(b.apply(a)), 1);
    assert_eq!(thread(&b), b.provider_id(m));
    b.sync();
    b.verify("no thread headers");
}

/// One message in a user folder refuses the whole action before any
/// write: a `failed` ledger entry with no message, and no `UID MOVE`.
#[test]
fn move_from_other_folder_refuses() {
    let fake = server(Caps::ALL);
    let (u, w) = (add(&fake, "INBOX", 1), add(&fake, "Work", 2));
    let b = setup("other", fake);
    let ms = [b.row("INBOX", u), b.row("Work", w)];
    let a = b.approved("archive", &ms);
    let e = failed(b.apply(a));
    assert!(e.contains("notSupported") || e.contains("cannot"), "{e}");
    assert_eq!(b.moves_sent(), 0);
    let (entries, rows): (i64, i64) = (
        b.one(
            "SELECT count(*) FROM ledger WHERE action_id = ?1 AND state = 'failed'",
            [a],
        ),
        b.one(
            "SELECT count(*) FROM ledger_message lm JOIN ledger l ON l.id = lm.ledger_id
             WHERE l.action_id = ?1",
            [a],
        ),
    );
    assert_eq!((entries, rows), (1, 0));
    let state: String = b.one("SELECT state FROM changeset_action WHERE id = ?1", [a]);
    assert_eq!(state, "failed");
}

/// A message that is already at the target is skipped, with no ledger row.
#[test]
fn move_already_there() {
    let fake = server(Caps::ALL);
    let (u, r) = (add(&fake, "INBOX", 1), add(&fake, "Archive", 2));
    let b = setup("there", fake);
    let ms = [b.row("INBOX", u), b.row("Archive", r)];
    let a = b.approved("archive", &ms);
    match b.apply(a) {
        Outcome::Applied(c) => assert_eq!((c.changed, c.unchanged), (1, 1)),
        o => panic!("{o:?}"),
    }
    assert_eq!(b.listed(a).len(), 1);
    b.sync();
    b.verify("already there");
}

/// A lost connection during the plan read: no ledger entry, the action
/// stays approved, and nothing moves. The next apply moves the message.
#[test]
fn move_plan_transport_error() {
    // The LIST of the plan is the last LIST before the first UID MOVE, less
    // the LIST of `relocate`: count them on a twin server.
    let lists = {
        let fake = server(Caps::ALL);
        let u = add(&fake, "INBOX", 1);
        let b = setup("plan-twin", fake);
        let before = b.fake.commands().len();
        let a = b.approved("archive", &[b.row("INBOX", u)]);
        assert_eq!(changed(b.apply(a)), 1);
        let cmds = b.fake.commands();
        let first_move = cmds.iter().position(|c| c.contains("UID MOVE")).unwrap();
        cmds[before..first_move]
            .iter()
            .filter(|c| c.split(' ').nth(1) == Some("LIST"))
            .count()
    };
    assert!(lists >= 2, "{lists}");
    let fake = server(Caps::ALL);
    let u = add(&fake, "INBOX", 1);
    let b = setup("plan", fake);
    let m = b.row("INBOX", u);
    let a = b.approved("archive", &[m]);
    b.fake.close_at("LIST", lists - 1);
    let r = b.try_apply(a);
    assert!(r.is_err(), "{r:?}");
    assert_eq!(b.moves_sent(), 0);
    let entries: i64 = b.one("SELECT count(*) FROM ledger WHERE action_id = ?1", [a]);
    assert_eq!(entries, 0);
    let state: String = b.one("SELECT state FROM changeset_action WHERE id = ?1", [a]);
    assert_eq!(state, "approved");
    assert_eq!(changed(b.apply(a)), 1);
}

/// Issue #984: a lost or incomplete answer to a move that the provider
/// confirms by a read rebinds each row, as a normal answer: the entry is
/// applied, a sync adds no row, and the undo moves each message back.
#[test]
fn a_confirmed_lost_answer_rebinds() {
    for mode in [Moving::Close, Moving::NoCopyuid] {
        let fake = server(Caps::ALL);
        let (ua, ub) = (add(&fake, "INBOX", 1), add(&fake, "INBOX", 2));
        let b = setup(&format!("confirmed-{mode:?}"), fake);
        let ms = [b.row("INBOX", ua), b.row("INBOX", ub)];
        let rows = b.rows();
        b.fake.set_moving(mode);
        let a = b.approved("archive", &ms);
        assert_eq!(changed(b.apply(a)), 2, "{mode:?}");
        assert_eq!(b.moves_sent(), 1, "{mode:?}");
        for &m in &ms {
            assert!(b.provider_id(m).ends_with(":Archive"), "{mode:?}");
        }
        b.fake.set_moving(Moving::Normal);
        b.sync();
        assert_eq!(b.rows(), rows, "{mode:?}: no new row, no gone row");
        b.verify(&format!("{mode:?}"));
        let outcomes = b.undo(a);
        assert!(
            outcomes.iter().all(|o| matches!(o, Outcome::Applied(_))),
            "{mode:?}: {outcomes:?}"
        );
        for &m in &ms {
            assert!(b.provider_id(m).ends_with(":INBOX"), "{mode:?}");
        }
    }
}

/// An unknown result of `UID MOVE` that a read cannot confirm (here a
/// part move) ends `failed` with each started row `unknown`: no second
/// `UID MOVE` and no rebind, whether the message moved (old ID not
/// found) or not (old ID with its labels).
#[test]
fn move_unknown_is_not_retried() {
    {
        let mode = Moving::RefuseAfterOne;
        let fake = server(Caps::ALL);
        let (ua, ub) = (add(&fake, "INBOX", 1), add(&fake, "INBOX", 2));
        let b = setup(&format!("unknown-{mode:?}"), fake);
        let ms = [b.row("INBOX", ua), b.row("INBOX", ub)];
        let old: Vec<String> = ms.iter().map(|&m| b.provider_id(m)).collect();
        b.fake.set_moving(mode);
        let a = b.approved("archive", &ms);
        failed(b.apply(a));
        assert_eq!(b.moves_sent(), 1, "{mode:?}");
        assert_eq!(
            b.listed(a),
            [Some("unknown".to_string()), Some("unknown".to_string())],
            "{mode:?}"
        );
        let now: Vec<String> = ms.iter().map(|&m| b.provider_id(m)).collect();
        assert_eq!(now, old, "{mode:?}: no rebind");
        b.fake.set_moving(Moving::Normal);
        b.sync();
        b.verify(&format!("{mode:?}"));
        assert_eq!(b.moves_sent(), 1, "{mode:?}: no second UID MOVE");
    }
}

/// A store error after `UID MOVE` (here, the rebind finds the new ID on
/// another row) stops the apply with the entry in `writing`, as a crash.
/// A sync then marks the old row gone and adds a row for the new ID. The
/// recovery gives `unknown` and never moves again; zero differences.
#[test]
fn move_crash_then_sync() {
    let fake = server(Caps::ALL);
    let u = add(&fake, "INBOX", 1);
    let b = setup("crash", fake);
    let m = b.row("INBOX", u);
    // The first UID of the empty archive folder.
    let new = format!("{}:1:Archive", b.fake.snapshot("Archive").0);
    let squat = b
        .store
        .write(|tx| {
            tx.execute(
                "INSERT INTO message(account_id, thread_id, provider_id)
                 SELECT account_id, thread_id, ?2 FROM message WHERE id = ?1",
                params![m, new],
            )?;
            Ok(tx.last_insert_rowid())
        })
        .unwrap();
    let a = b.approved("archive", &[m]);
    let r = b.try_apply(a);
    assert!(
        matches!(
            &r,
            Err(iris_apply::Error::Store(iris_store::Error::Taken(_)))
        ),
        "{r:?}"
    );
    assert_eq!(b.moves_sent(), 1);
    let state: String = b.one("SELECT state FROM ledger WHERE action_id = ?1", [a]);
    assert_eq!(state, "writing");
    b.store
        .write(|tx| Ok(tx.execute("DELETE FROM message WHERE id = ?1", [squat])?))
        .unwrap();
    b.sync();
    let gone: Option<i64> = b.one("SELECT gone_at FROM message WHERE id = ?1", [m]);
    assert!(gone.is_some(), "the sync marks the old row gone");
    failed(b.apply(a));
    assert_eq!(b.listed(a), [Some("unknown".to_string())]);
    assert_eq!(b.moves_sent(), 1);
    b.verify("crash then sync");
}

/// Issue #990: after an archive, another client moves one message to the
/// trash. The undo (`inbox`) counts that message gone (its archive ID is
/// not found), never refuses the action, and moves the other message back.
#[test]
fn undo_after_an_external_move() {
    for sync_first in [false, true] {
        let fake = server(Caps::ALL);
        let (ua, ub) = (add(&fake, "INBOX", 1), add(&fake, "INBOX", 2));
        let b = setup(&format!("external-{sync_first}"), fake);
        let ms = [b.row("INBOX", ua), b.row("INBOX", ub)];
        let a = b.approved("archive", &ms);
        assert_eq!(changed(b.apply(a)), 2);
        // The other client: a move of the first message from the archive to the trash.
        let id = b.provider_id(ms[0]);
        let uid: u32 = id.split(':').nth(1).unwrap().parse().unwrap();
        b.fake.remove("Archive", uid);
        add(&b.fake, "Trash", 1);
        if sync_first {
            b.sync();
        }
        let outcomes = b.undo(a);
        match outcomes.as_slice() {
            // After a sync, the undo leaves out the gone row.
            [Outcome::Applied(c)] => assert_eq!(c.changed, 1, "{sync_first}: {c:?}"),
            o => panic!("{sync_first}: {o:?}"),
        }
        assert!(b.provider_id(ms[1]).ends_with(":INBOX"));
        b.sync();
        b.verify(&format!("external move, sync first {sync_first}"));
    }
}
