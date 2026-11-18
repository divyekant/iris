//! The IMAP provider against the fake IMAP server: the mirror of iris-sync
//! has zero differences after each change, and the failure list of the spec
//! (docs/specs/2026-11-17-imap-provider.md) holds. No network.

use std::collections::BTreeSet;
use std::net::TcpStream;
use std::path::PathBuf;
use std::time::Duration;

use iris_fakeimap::{Caps, FakeImap, PASSWORD, USER};
use iris_imap::{Imap, SENT_COPY, UNSENT};
use iris_provider::{Error, Provider};
use iris_store::Store;
use iris_sync::{first_sync, sync, verify};
use iris_synth::Mailbox;

const OWNER: &str = "owner@example.org";

fn provider(fake: &FakeImap) -> Imap<TcpStream> {
    let addr = fake.addr();
    Imap::new(OWNER, USER, move || {
        let s = TcpStream::connect(addr)?;
        s.set_nodelay(true)?;
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
fn no_uidnext() {
    let condstore_only = Caps {
        qresync: false,
        uidnext: false,
        ..Caps::ALL
    };
    let (fake, _) = filled(condstore_only, 2);
    let r = provider(&fake).profile(PASSWORD);
    assert!(
        matches!(&r, Err(Error::Api { status: 400, reason, .. }) if reason == "noUidnext"),
        "{r:?}"
    );
    // With QRESYNC, VANISHED covers removals and UIDNEXT does not matter.
    let qresync = Caps {
        uidnext: false,
        ..Caps::ALL
    };
    let (fake, _) = filled(qresync, 2);
    assert!(provider(&fake).profile(PASSWORD).is_ok());
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

/// All changes from `from`, flat.
fn change_list(mail: &dyn Provider, from: &str) -> Vec<iris_provider::Change> {
    let mut page = None;
    let mut out = Vec::new();
    loop {
        let p = mail.changes(PASSWORD, from, page.as_deref()).unwrap();
        out.extend(p.history.into_iter().flat_map(|h| h.changes));
        match p.next_page_token {
            Some(t) => page = Some(t),
            None => return out,
        }
    }
}

#[test]
fn came_and_went_after_the_cursor() {
    for caps in [
        Caps::ALL,
        Caps {
            qresync: false,
            ..Caps::ALL
        },
    ] {
        let (fake, _) = filled(caps, 3);
        let mail = provider(&fake);
        let c = cursor(&mail);
        // The first sync can list this message after the cursor.
        let uid = fake.add("INBOX", Mailbox::new(14, 1).raw(0), &[]);
        fake.remove("INBOX", uid);
        let deleted: Vec<String> = change_list(&mail, &c)
            .into_iter()
            .filter_map(|ch| match ch {
                iris_provider::Change::Deleted(m) => Some(m.id),
                _ => None,
            })
            .collect();
        assert!(
            deleted
                .iter()
                .any(|id| id.ends_with(&format!(":{uid}:INBOX"))),
            "{caps:?}: {deleted:?}"
        );
    }
}

/// The invariant of the mirror: after each sync, verify finds zero
/// differences, for a random run of adds, removals and flag changes.
#[test]
fn random_changes_keep_zero_differences() {
    for (seed, caps) in [
        (3u64, Caps::ALL),
        (
            5,
            Caps {
                qresync: false,
                ..Caps::ALL
            },
        ),
    ] {
        let (fake, mut live) = filled(caps, 10);
        let mail = provider(&fake);
        let store = Store::open(&data_dir(&format!("random-{seed}"))).unwrap();
        let account = store.add_account("imap", OWNER).unwrap();
        first_sync(&store, &mail, account, PASSWORD).unwrap();
        let extra = Mailbox::new(seed, 40);
        let flags: [&[&str]; 4] = [&[], &["\\Seen"], &["\\Flagged", "\\Draft"], &["$K"]];
        let mut rng = seed;
        let mut next = || {
            rng ^= rng << 13;
            rng ^= rng >> 7;
            rng ^= rng << 17;
            rng
        };
        let mut used = 0;
        for _round in 0..6 {
            for _ in 0..4 {
                match next() % 3 {
                    0 if used < extra.len() => {
                        let folder = ["INBOX", "Archive", "Sent"][(next() % 3) as usize];
                        let f = flags[(next() % 4) as usize];
                        live.push((folder, fake.add(folder, extra.raw(used), f)));
                        used += 1;
                    }
                    1 if !live.is_empty() => {
                        let (f, uid) = live.remove((next() % live.len() as u64) as usize);
                        fake.remove(f, uid);
                    }
                    _ if !live.is_empty() => {
                        let (f, uid) = live[(next() % live.len() as u64) as usize];
                        fake.set_flags(f, uid, flags[(next() % 4) as usize]);
                    }
                    _ => {}
                }
            }
            sync(&store, &mail, account, PASSWORD).unwrap();
            assert_clean(&store, &mail, account);
        }
    }
}

#[test]
fn labels_of_special_folders_and_flags() {
    let fake = FakeImap::start(Caps::ALL);
    fake.create("Sent", Some("\\Sent"));
    fake.create("Drafts", Some("\\Drafts"));
    let sent = fake.add("Sent", Mailbox::new(15, 1).raw(0), &["\\Seen"]);
    let draft = fake.add("INBOX", Mailbox::new(16, 1).raw(0), &["\\Seen", "\\Draft"]);
    let in_drafts = fake.add("Drafts", Mailbox::new(17, 1).raw(0), &["\\Seen", "\\Draft"]);
    let mail = provider(&fake);
    let validity = |f: &str| {
        let c = cursor(&mail);
        c.lines()
            .find(|l| l.ends_with(&format!(" {f}")))
            .unwrap()
            .split(' ')
            .next()
            .unwrap()
            .to_owned()
    };
    let labels = |id: String| mail.minimal(PASSWORD, &id).unwrap().label_ids;
    // No IMAP message gets SENT (spec section 4).
    // SENT_COPY keeps it out of the understanding of received mail.
    assert_eq!(
        labels(format!("{}:{sent}:Sent", validity("Sent"))),
        vec!["folder:Sent", SENT_COPY]
    );
    let copies = mail.messages(PASSWORD, Some(SENT_COPY), None).unwrap();
    assert_eq!(copies.messages.len(), 1);
    // Each folder label is system: a label action cannot target a folder.
    for l in mail.labels(PASSWORD).unwrap() {
        if l.id.starts_with("folder:") {
            assert_eq!(l.kind, "system", "{l:?}");
        }
    }
    // Nor DRAFT: a folder or a flag is not proof that the owner wrote it.
    // UNSENT keeps the draft out of the understanding.
    assert_eq!(
        labels(format!("{}:{draft}:INBOX", validity("INBOX"))),
        vec!["INBOX", UNSENT]
    );
    assert_eq!(
        labels(format!("{}:{in_drafts}:Drafts", validity("Drafts"))),
        vec!["folder:Drafts", UNSENT]
    );
    let unsent = mail.messages(PASSWORD, Some(UNSENT), None).unwrap();
    let mut ids: Vec<String> = unsent.messages.into_iter().map(|m| m.id).collect();
    let mut page = unsent.next_page_token;
    while let Some(p) = page {
        let next = mail.messages(PASSWORD, Some(UNSENT), Some(&p)).unwrap();
        ids.extend(next.messages.into_iter().map(|m| m.id));
        page = next.next_page_token;
    }
    assert_eq!(ids.len(), 2, "{ids:?}");
    let ids: Vec<String> = mail
        .labels(PASSWORD)
        .unwrap()
        .into_iter()
        .map(|l| l.id)
        .collect();
    assert!(!ids.iter().any(|i| i == "SENT" || i == "DRAFT"), "{ids:?}");
    // A label that labels() does not give is never reported as made.
    assert!(matches!(
        mail.create_label(PASSWORD, "Work"),
        Err(Error::Api { status: 400, .. })
    ));
}

fn small(i: usize) -> Vec<u8> {
    format!("Message-ID: <m{i}@x>\r\nSubject: {i}\r\n\r\nbody {i}\r\n").into_bytes()
}

/// A page of changes for a new folder holds at most 2,000 records, and the
/// sync of a folder of more than one page is still exact.
#[test]
fn changes_pages() {
    for caps in [
        Caps::ALL,
        Caps {
            qresync: false,
            ..Caps::ALL
        },
    ] {
        let (fake, _) = filled(caps, 3);
        let mail = provider(&fake);
        let store = Store::open(&data_dir(&format!("pages-{}", caps.qresync))).unwrap();
        let account = store.add_account("imap", OWNER).unwrap();
        first_sync(&store, &mail, account, PASSWORD).unwrap();
        let c = cursor(&mail);
        fake.create("Big", None);
        let uids: Vec<u32> = (0..2100).map(|i| fake.add("Big", small(i), &[])).collect();
        let mut page = None;
        let mut added = BTreeSet::new();
        loop {
            let p = mail.changes(PASSWORD, &c, page.as_deref()).unwrap();
            let n: usize = p.history.iter().map(|h| h.changes.len()).sum();
            assert!(n <= 2000, "{n} records in one page");
            for ch in p.history.into_iter().flat_map(|h| h.changes) {
                if let iris_provider::Change::Added(m) = ch {
                    added.insert(m.id);
                }
            }
            match p.next_page_token {
                Some(t) => page = Some(t),
                None => break,
            }
        }
        assert_eq!(added.len(), 2100);
        sync(&store, &mail, account, PASSWORD).unwrap();
        assert_clean(&store, &mail, account);
        // A flag change and a removal in a known folder.
        fake.remove("Big", uids[5]);
        for u in uids.iter().skip(6).take(50) {
            fake.set_flags("Big", *u, &["\\Flagged"]);
        }
        sync(&store, &mail, account, PASSWORD).unwrap();
        assert_clean(&store, &mail, account);
    }
}

/// The message pages read each folder once, INBOX first, also when a
/// folder comes before the current one in the order between two calls.
#[test]
fn message_pages_follow_folder_names() {
    let fake = FakeImap::start(Caps::ALL);
    fake.create("b", None);
    fake.add("INBOX", small(0), &[]);
    fake.add("b", small(1), &[]);
    let mail = provider(&fake);
    let first = mail.messages(PASSWORD, None, None).unwrap();
    assert!(first.messages[0].id.ends_with(":INBOX"));
    // A folder made before the current one in the order is not in this
    // sequence; the cursor of a later changes call gets it.
    fake.create("a", None);
    fake.add("a", small(2), &[]);
    let second = mail
        .messages(PASSWORD, None, first.next_page_token.as_deref())
        .unwrap();
    assert!(second.messages[0].id.ends_with(":b"), "{second:?}");
    assert_eq!(second.next_page_token, None);
}

/// A label search window names sequence numbers, not UIDs: a folder whose
/// UIDs do not start at 1 still lists each match.
#[test]
fn search_windows_use_sequence_numbers() {
    let fake = FakeImap::start(Caps::ALL);
    let uids: Vec<u32> = (0..30).map(|i| fake.add("INBOX", small(i), &[])).collect();
    // UIDs 1 to 20 go: the 10 left have UIDs 21 to 30 and sequence numbers 1 to 10.
    for u in &uids[..20] {
        fake.remove("INBOX", *u);
    }
    fake.set_flags("INBOX", uids[25], &["\\Flagged"]);
    let mail = provider(&fake);
    let page = mail.messages(PASSWORD, Some("STARRED"), None).unwrap();
    assert_eq!(page.messages.len(), 1, "{page:?}");
    assert!(page.messages[0].id.contains(&format!(":{}:", uids[25])));
}

/// Two `\Junk` folders both give SPAM, and the two spellings of one
/// keyword give one label (spec section 4). The mirror stays clean.
#[test]
fn two_junk_folders_and_keyword_case() {
    let fake = FakeImap::start(Caps::ALL);
    fake.create("Junk", Some("\\Junk"));
    fake.create("Spam", Some("\\Junk"));
    fake.add("Junk", Mailbox::new(21, 1).raw(0), &["\\Seen", "$Todo"]);
    fake.add(
        "Spam",
        Mailbox::new(22, 1).raw(0),
        &["\\Seen", "$todo", "$TODO"],
    );
    let mail = provider(&fake);

    let spam = mail.messages(PASSWORD, Some("SPAM"), None).unwrap();
    let mut n = spam.messages.len();
    let mut page = spam.next_page_token;
    while let Some(p) = page {
        let next = mail.messages(PASSWORD, Some("SPAM"), Some(&p)).unwrap();
        n += next.messages.len();
        page = next.next_page_token;
    }
    assert_eq!(n, 2);

    let kws: Vec<_> = mail
        .labels(PASSWORD)
        .unwrap()
        .into_iter()
        .filter(|l| l.id.starts_with("kw:"))
        .collect();
    assert_eq!(kws.len(), 1, "{kws:?}");
    assert_eq!(kws[0].id, "kw:$todo");
    let tagged = mail.messages(PASSWORD, Some("kw:$todo"), None).unwrap();
    let mut n = tagged.messages.len();
    let mut page = tagged.next_page_token;
    while let Some(p) = page {
        let next = mail.messages(PASSWORD, Some("kw:$todo"), Some(&p)).unwrap();
        n += next.messages.len();
        page = next.next_page_token;
    }
    assert_eq!(n, 2);

    let store = Store::open(&data_dir("junk-case")).unwrap();
    let account = store.add_account("imap", OWNER).unwrap();
    first_sync(&store, &mail, account, PASSWORD).unwrap();
    assert_clean(&store, &mail, account);
}

/// A server with no special-use attributes: the common folder names give
/// the special labels (spec section 4), and the mirror stays clean.
#[test]
fn special_folders_by_name() {
    let fake = FakeImap::start(Caps::ALL);
    for f in ["Sent Items", "Drafts", "Spam", "Trash", "Projects"] {
        fake.create(f, None);
    }
    let mail = provider(&fake);
    let mut seen = Vec::new();
    for (i, f) in ["Sent Items", "Drafts", "Spam", "Trash", "Projects"]
        .iter()
        .enumerate()
    {
        let uid = fake.add(f, Mailbox::new(30 + i as u64, 1).raw(0), &["\\Seen"]);
        let validity = cursor(&mail)
            .lines()
            .find(|l| l.ends_with(&format!(" {f}")))
            .unwrap()
            .split(' ')
            .next()
            .unwrap()
            .to_owned();
        let id = format!("{validity}:{uid}:{f}");
        seen.push(mail.minimal(PASSWORD, &id).unwrap().label_ids);
    }
    assert_eq!(seen[0], vec!["folder:Sent Items", SENT_COPY]);
    assert_eq!(seen[1], vec!["folder:Drafts", UNSENT]);
    assert_eq!(seen[2], vec!["SPAM"]);
    assert_eq!(seen[3], vec!["TRASH"]);
    assert_eq!(seen[4], vec!["folder:Projects"]);

    let store = Store::open(&data_dir("by-name")).unwrap();
    let account = store.add_account("imap", OWNER).unwrap();
    first_sync(&store, &mail, account, PASSWORD).unwrap();
    assert_clean(&store, &mail, account);
}

#[test]
fn an_attribute_wins_over_a_name() {
    let fake = FakeImap::start(Caps::ALL);
    fake.create("Junk", Some("\\Junk"));
    fake.create("Spam", None);
    let uid = fake.add("Spam", Mailbox::new(40, 1).raw(0), &["\\Seen"]);
    let mail = provider(&fake);
    let validity = cursor(&mail)
        .lines()
        .find(|l| l.ends_with(" Spam"))
        .unwrap()
        .split(' ')
        .next()
        .unwrap()
        .to_owned();
    let labels = mail
        .minimal(PASSWORD, &format!("{validity}:{uid}:Spam"))
        .unwrap()
        .label_ids;
    assert_eq!(labels, vec!["folder:Spam"]);
}

/// A new `\Junk` folder takes the role from a folder named "Spam": its
/// messages lose SPAM with no change of their own (spec section 5).
#[test]
fn folder_role_change() {
    let fake = FakeImap::start(Caps::ALL);
    fake.create("Spam", None);
    fake.add("Spam", Mailbox::new(50, 1).raw(0), &["\\Seen"]);
    fake.add("INBOX", Mailbox::new(51, 1).raw(0), &[]);
    let mail = provider(&fake);
    let store = Store::open(&data_dir("role")).unwrap();
    let account = store.add_account("imap", OWNER).unwrap();
    first_sync(&store, &mail, account, PASSWORD).unwrap();
    assert_clean(&store, &mail, account);
    let before = cursor(&mail);

    fake.create("Junk", Some("\\Junk"));
    assert!(matches!(
        all_changes(&mail, &before),
        Err(Error::HistoryExpired)
    ));
    sync(&store, &mail, account, PASSWORD).unwrap();
    assert_clean(&store, &mail, account);
}
