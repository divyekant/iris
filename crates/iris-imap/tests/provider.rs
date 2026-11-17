//! The IMAP provider against the fake IMAP server: the mirror of iris-sync
//! has zero differences after each change, and the failure list of the spec
//! (docs/specs/2026-11-17-imap-provider.md) holds. No network.

use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;

use iris_fakeimap::{Caps, FakeImap, PASSWORD, USER};
use iris_imap::Imap;
use iris_provider::{Error, Provider};
use iris_store::Store;
use iris_sync::{first_sync, sync, verify};
use iris_synth::Mailbox;

const OWNER: &str = "owner@example.org";

fn provider(fake: &FakeImap) -> Imap<TcpStream> {
    let addr = fake.addr();
    Imap::new(OWNER, USER, move || {
        let s = TcpStream::connect(addr)?;
        s.set_read_timeout(Some(Duration::from_secs(10)))?;
        Ok(s)
    })
}

fn data_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("iris-imap-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

/// A fake with four folders and `n` synthetic messages, spread over the
/// folders with a mix of flags. Gives the (folder, UID) of each message.
fn filled(caps: Caps, n: usize) -> (FakeImap, Vec<(&'static str, u32)>) {
    let fake = FakeImap::start(caps);
    fake.create("Archive", None);
    fake.create("Sent", Some("\\Sent"));
    fake.create("Drafts", Some("\\Drafts"));
    let mailbox = Mailbox::new(11, n);
    let folders = ["INBOX", "Archive", "Sent", "Drafts", "INBOX"];
    let flags: [&[&str]; 4] = [&[], &["\\Seen"], &["\\Seen", "\\Flagged"], &["$Work"]];
    let mut out = Vec::new();
    for i in 0..n {
        let folder = folders[i % folders.len()];
        out.push((
            folder,
            fake.add(folder, mailbox.raw(i), flags[i % flags.len()]),
        ));
    }
    (fake, out)
}

fn assert_clean(store: &Store, mail: &dyn Provider, account: i64) {
    let report = verify(store, mail, account, PASSWORD).unwrap();
    assert_eq!(report.differences, vec![], "differences: {report:?}");
    assert_eq!(report.total, 0);
}

fn count(store: &Store, sql: &str) -> i64 {
    store
        .read(|c| Ok(c.query_row(sql, [], |r| r.get(0))?))
        .unwrap()
}

/// The card test: sync and verify, then each kind of change, with zero
/// differences each time. Runs in QRESYNC mode and in CONDSTORE-only mode.
fn zero_differences(caps: Caps, name: &str) {
    let (fake, uids) = filled(caps, 40);
    let mail = provider(&fake);
    let store = Store::open(&data_dir(name)).unwrap();
    let account = store.add_account("imap", OWNER).unwrap();

    let done = first_sync(&store, &mail, account, PASSWORD).unwrap();
    assert_eq!(done.fetched, 40);
    assert_clean(&store, &mail, account);
    // No IMAP message gets SENT (spec section 4).
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM message_label WHERE label_id = 'SENT'"
        ),
        0
    );

    // Flag changes and a new message.
    let (f, uid) = uids[0];
    fake.set_flags(f, uid, &["\\Seen", "\\Flagged", "$Late"]);
    fake.add("Archive", Mailbox::new(12, 1).raw(0), &[]);
    sync(&store, &mail, account, PASSWORD).unwrap();
    assert_clean(&store, &mail, account);

    // A removal and a new folder.
    let (f, uid) = uids[1];
    fake.remove(f, uid);
    fake.create("Later", None);
    fake.add("Later", Mailbox::new(13, 1).raw(0), &["\\Seen"]);
    sync(&store, &mail, account, PASSWORD).unwrap();
    assert_clean(&store, &mail, account);

    // A new UIDVALIDITY and a deleted folder start a full resync.
    fake.renew("Archive");
    fake.drop_folder("Later");
    sync(&store, &mail, account, PASSWORD).unwrap();
    assert_clean(&store, &mail, account);
}

#[test]
fn zero_differences_qresync() {
    zero_differences(Caps::ALL, "qresync");
}

#[test]
fn zero_differences_condstore_only() {
    let caps = Caps {
        qresync: false,
        ..Caps::ALL
    };
    zero_differences(caps, "condstore");
}

fn cursor(mail: &dyn Provider) -> String {
    mail.profile(PASSWORD).unwrap().cursor
}

/// All pages of the changes from `from`: the first error, or the new cursor.
fn all_changes(mail: &dyn Provider, from: &str) -> Result<String, Error> {
    let mut page = None;
    loop {
        let p = mail.changes(PASSWORD, from, page.as_deref())?;
        match p.next_page_token {
            Some(t) => page = Some(t),
            None => return Ok(p.cursor),
        }
    }
}

#[test]
fn bad_id() {
    let (fake, _) = filled(Caps::ALL, 2);
    let mail = provider(&fake);
    for id in ["x", "1:2", "a:1:INBOX", "1:b:INBOX", "1:1:NoSuchFolder"] {
        let r = mail.minimal(PASSWORD, id);
        assert!(
            matches!(r, Err(Error::NotFound) | Err(Error::Api { .. })),
            "{id}: {r:?}"
        );
    }
    // A known folder with another UIDVALIDITY.
    assert!(matches!(
        mail.minimal(PASSWORD, "1:1:INBOX"),
        Err(Error::NotFound)
    ));
}

#[test]
fn bad_cursor() {
    let (fake, _) = filled(Caps::ALL, 2);
    let mail = provider(&fake);
    for c in ["", "x", "1 2 3", "1 2 3 4"] {
        let r = all_changes(&mail, c);
        assert!(matches!(r, Err(Error::HistoryExpired)), "{c:?}: {r:?}");
    }
}

#[test]
fn uidvalidity_change() {
    let (fake, _) = filled(Caps::ALL, 4);
    let mail = provider(&fake);
    let c = cursor(&mail);
    fake.renew("INBOX");
    assert!(matches!(all_changes(&mail, &c), Err(Error::HistoryExpired)));
}

#[test]
fn folder_gone() {
    let (fake, _) = filled(Caps::ALL, 4);
    let mail = provider(&fake);
    let c = cursor(&mail);
    fake.drop_folder("Archive");
    assert!(matches!(all_changes(&mail, &c), Err(Error::HistoryExpired)));
}

#[test]
fn removal_condstore() {
    let caps = Caps {
        qresync: false,
        ..Caps::ALL
    };
    let (fake, uids) = filled(caps, 4);
    let mail = provider(&fake);
    let c = cursor(&mail);
    let (f, uid) = uids[0];
    fake.remove(f, uid);
    // Without QRESYNC, no answer names the removed UID (issue #630).
    assert!(matches!(all_changes(&mail, &c), Err(Error::HistoryExpired)));
}

#[test]
fn no_condstore() {
    let caps = Caps {
        condstore: false,
        qresync: false,
        ..Caps::ALL
    };
    let (fake, _) = filled(caps, 2);
    let mail = provider(&fake);
    let r = mail.profile(PASSWORD);
    assert!(
        matches!(&r, Err(Error::Api { status: 400, reason, .. }) if reason == "noCondstore"),
        "{r:?}"
    );
}

#[test]
fn wrong_password() {
    let (fake, _) = filled(Caps::ALL, 1);
    let mail = provider(&fake);
    let r = mail.profile("not-the-password");
    assert!(matches!(r, Err(Error::Unauthorized)), "{r:?}");
    // The error and the server log do not hold the secret.
    assert!(!format!("{r:?}").contains("not-the-password"));
    assert!(fake
        .commands()
        .iter()
        .all(|c| !c.contains("not-the-password")));
    // The next call signs in again.
    assert!(mail.profile(PASSWORD).is_ok());
}

#[test]
fn writes_refuse_before_any_command() {
    let (fake, _) = filled(Caps::ALL, 1);
    let mail = provider(&fake);
    let before = fake.commands().len();
    let r = mail.modify(PASSWORD, &["1001:1:INBOX"], &["UNREAD"], &[]);
    assert!(matches!(r, Err(Error::Api { status: 400, .. })), "{r:?}");
    assert!(matches!(
        mail.trash(PASSWORD, "1001:1:INBOX"),
        Err(Error::Api { status: 400, .. })
    ));
    assert_eq!(fake.commands().len(), before);
}
