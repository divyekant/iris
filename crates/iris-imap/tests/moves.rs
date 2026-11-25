//! Moves of the IMAP provider (docs/specs/2026-11-25-imap-moves.md, the
//! failure list) against the fake IMAP server. No network.

use std::net::TcpStream;
use std::time::Duration;

use iris_fakeimap::{Caps, FakeImap, Moving, PASSWORD, USER};
use iris_imap::Imap;
use iris_provider::{Error, Move, Provider};
use iris_synth::Mailbox;

fn provider(fake: &FakeImap) -> Imap<TcpStream> {
    let addr = fake.addr();
    Imap::new("owner@example.org", USER, move || {
        let s = TcpStream::connect(addr)?;
        s.set_read_timeout(Some(Duration::from_secs(10)))?;
        Ok(s)
    })
}

/// The ID of `uid` in `folder`: `<UIDVALIDITY>:<uid>:<folder>`.
fn id(fake: &FakeImap, folder: &str, uid: u32) -> String {
    format!("{}:{uid}:{folder}", fake.snapshot(folder).0)
}

fn add(fake: &FakeImap, folder: &str, n: u64) -> String {
    let uid = fake.add(folder, Mailbox::new(n, 1).raw(0), &["\\Seen"]);
    id(fake, folder, uid)
}

fn reason(r: Result<impl std::fmt::Debug, Error>) -> String {
    match r {
        Err(Error::Api {
            status: 400,
            reason,
            ..
        }) => reason,
        r => panic!("not a definite refusal: {r:?}"),
    }
}

fn moves_sent(fake: &FakeImap) -> usize {
    fake.commands()
        .iter()
        .filter(|c| c.contains("UID MOVE"))
        .count()
}

/// A server with `\Archive` and `\Trash` folders.
fn server() -> FakeImap {
    let fake = FakeImap::start(Caps::ALL);
    fake.create("Archive", Some("\\Archive"));
    fake.create("Trash", Some("\\Trash"));
    fake
}

/// archive, inbox, trash and untrash: each move gives the new location
/// ID, the old ID is gone, and the new ID has the labels of the target.
#[test]
fn moves_give_new_ids() {
    for tagged in [false, true] {
        let fake = server();
        if tagged {
            fake.set_moving(Moving::Tagged);
        }
        let mail = provider(&fake);
        let a = add(&fake, "INBOX", 1);
        let b = add(&fake, "INBOX", 2);
        let ids = [a.as_str(), b.as_str()];
        assert_eq!(
            mail.move_plan(PASSWORD, &ids, Move::Archive).unwrap(),
            [true, true]
        );
        let new = mail.relocate(PASSWORD, &ids, Move::Archive).unwrap();
        assert_eq!(new.len(), 2);
        assert!(new.iter().all(|n| n.ends_with(":Archive")), "{new:?}");
        assert_ne!(new[0], new[1]);
        assert!(matches!(mail.minimal(PASSWORD, &a), Err(Error::NotFound)));
        assert_eq!(
            mail.minimal(PASSWORD, &new[0]).unwrap().label_ids,
            ["folder:Archive"]
        );
        assert!(fake.uids("INBOX").is_empty());

        let back = mail
            .relocate(PASSWORD, &[new[0].as_str()], Move::Inbox)
            .unwrap();
        assert_eq!(
            mail.minimal(PASSWORD, &back[0]).unwrap().label_ids,
            ["INBOX"]
        );
        let trashed = mail
            .relocate(PASSWORD, &[back[0].as_str()], Move::Trash)
            .unwrap();
        assert_eq!(
            mail.minimal(PASSWORD, &trashed[0]).unwrap().label_ids,
            ["TRASH"]
        );
        let out = mail
            .relocate(PASSWORD, &[trashed[0].as_str()], Move::Untrash)
            .unwrap();
        assert_eq!(
            mail.minimal(PASSWORD, &out[0]).unwrap().label_ids,
            ["INBOX"]
        );
        // trash from the archive folder too.
        let t = mail
            .relocate(PASSWORD, &[new[1].as_str()], Move::Trash)
            .unwrap();
        assert!(t[0].ends_with(":Trash"));
    }
}

#[test]
fn move_needs_move_and_uidplus() {
    for caps in [
        Caps {
            moves: false,
            ..Caps::ALL
        },
        Caps {
            uidplus: false,
            ..Caps::ALL
        },
    ] {
        let fake = FakeImap::start(caps);
        fake.create("Archive", Some("\\Archive"));
        let mail = provider(&fake);
        let a = add(&fake, "INBOX", 1);
        let r = mail.move_plan(PASSWORD, &[&a], Move::Archive);
        assert_eq!(reason(r), "notSupported");
        let r = mail.relocate(PASSWORD, &[&a], Move::Archive);
        assert_eq!(reason(r), "notSupported");
        assert_eq!(moves_sent(&fake), 0);
    }
}

/// A message that is done already skips in the plan, and `relocate`
/// refuses it: the caller must plan first.
#[test]
fn move_already_there() {
    let fake = server();
    let mail = provider(&fake);
    let archived = add(&fake, "Archive", 1);
    let inbox = add(&fake, "INBOX", 2);
    let trashed = add(&fake, "Trash", 3);
    let plan = |ids: &[&str], mv| mail.move_plan(PASSWORD, ids, mv).unwrap();
    assert_eq!(
        plan(&[&archived, &trashed, &inbox], Move::Archive),
        [false, false, true]
    );
    assert_eq!(plan(&[&inbox, &archived], Move::Inbox), [false, true]);
    assert_eq!(plan(&[&trashed, &inbox], Move::Trash), [false, true]);
    assert_eq!(
        plan(&[&inbox, &archived, &trashed], Move::Untrash),
        [false, false, true]
    );
    assert_eq!(
        reason(mail.relocate(PASSWORD, &[&archived], Move::Archive)),
        "notSupported"
    );
    assert_eq!(moves_sent(&fake), 0);
}

#[test]
fn move_without_folder() {
    let fake = FakeImap::start(Caps::ALL);
    let mail = provider(&fake);
    let a = add(&fake, "INBOX", 1);
    assert_eq!(
        reason(mail.move_plan(PASSWORD, &[&a], Move::Archive)),
        "noArchive"
    );
    assert_eq!(
        reason(mail.relocate(PASSWORD, &[&a], Move::Trash)),
        "noTrash"
    );
    assert_eq!(moves_sent(&fake), 0);
}

/// A skip needs no target folder (moves spec, section 1).
#[test]
fn move_skip_needs_no_target() {
    let fake = FakeImap::start(Caps::ALL);
    fake.create("Trash", Some("\\Trash"));
    let mail = provider(&fake);
    let a = add(&fake, "INBOX", 1);
    let t = add(&fake, "Trash", 2);
    assert_eq!(
        mail.move_plan(PASSWORD, &[&t], Move::Archive).unwrap(),
        [false]
    );
    assert_eq!(
        mail.move_plan(PASSWORD, &[&a], Move::Inbox).unwrap(),
        [false]
    );
    let fake = FakeImap::start(Caps::ALL);
    let mail = provider(&fake);
    let a = add(&fake, "INBOX", 1);
    assert_eq!(
        mail.move_plan(PASSWORD, &[&a], Move::Untrash).unwrap(),
        [false]
    );
}

#[test]
fn move_ambiguous_folder() {
    let fake = FakeImap::start(Caps::ALL);
    fake.create("Trash", Some("\\Trash"));
    fake.create("Bin", Some("\\Trash"));
    fake.create("Archive", Some("\\Archive"));
    fake.create("Old", Some("\\Archive"));
    let mail = provider(&fake);
    let a = add(&fake, "INBOX", 1);
    assert_eq!(
        reason(mail.move_plan(PASSWORD, &[&a], Move::Trash)),
        "ambiguousTrash"
    );
    assert_eq!(
        reason(mail.move_plan(PASSWORD, &[&a], Move::Archive)),
        "ambiguousArchive"
    );
    // A message in either trash folder is in the trash: the skip needs no target.
    let b = add(&fake, "Bin", 2);
    assert_eq!(
        mail.move_plan(PASSWORD, &[&b], Move::Trash).unwrap(),
        [false]
    );
    assert_eq!(moves_sent(&fake), 0);
}

/// One message in a user folder refuses the whole call, before any write.
#[test]
fn move_from_other_folder_refuses() {
    let fake = server();
    fake.create("Projects", None);
    fake.create("Sent", Some("\\Sent"));
    let mail = provider(&fake);
    let a = add(&fake, "INBOX", 1);
    for folder in ["Projects", "Sent"] {
        let p = add(&fake, folder, 2);
        for mv in [Move::Archive, Move::Inbox, Move::Trash, Move::Untrash] {
            assert_eq!(
                reason(mail.move_plan(PASSWORD, &[&a, &p], mv)),
                "notSupported"
            );
            assert_eq!(
                reason(mail.relocate(PASSWORD, &[&a, &p], mv)),
                "notSupported"
            );
        }
    }
    assert_eq!(moves_sent(&fake), 0);
    assert_eq!(fake.uids("INBOX").len(), 1);
}

/// The role of a folder follows the reads: with a `\Trash` folder, an
/// ordinary folder named `Trash` is a user folder.
#[test]
fn move_role_follows_the_attribute() {
    let fake = FakeImap::start(Caps::ALL);
    fake.create("Bin", Some("\\Trash"));
    fake.create("Trash", None);
    let mail = provider(&fake);
    let t = add(&fake, "Trash", 1);
    assert_eq!(
        reason(mail.move_plan(PASSWORD, &[&t], Move::Untrash)),
        "notSupported"
    );
    // With no attribute at all, the names give the roles.
    let fake = FakeImap::start(Caps::ALL);
    fake.create("Archiv", None);
    let mail = provider(&fake);
    let a = add(&fake, "INBOX", 1);
    let new = mail.relocate(PASSWORD, &[&a], Move::Archive).unwrap();
    assert!(new[0].ends_with(":Archiv"), "{new:?}");
}

#[test]
fn move_uidvalidity() {
    let fake = server();
    let mail = provider(&fake);
    let a = add(&fake, "INBOX", 1);
    fake.renew_at_select("INBOX", 1);
    assert!(matches!(
        mail.relocate(PASSWORD, &[&a], Move::Archive),
        Err(Error::NotFound)
    ));
    assert_eq!(moves_sent(&fake), 0);

    // A second folder renews after the first folder moved: unknown.
    let fake = server();
    let mail = provider(&fake);
    let a = add(&fake, "INBOX", 1);
    let b = add(&fake, "Archive", 2);
    let ids = [b.as_str(), a.as_str()];
    // Folders move in page order: INBOX first, then Archive.
    fake.renew_at_select("Archive", 2);
    let r = mail.relocate(PASSWORD, &ids, Move::Trash);
    assert!(matches!(&r, Err(e) if e.write_result_unknown()), "{r:?}");
    assert_eq!(moves_sent(&fake), 1);
    // INBOX moved; Archive did not.
    assert!(fake.uids("INBOX").is_empty());
    assert_eq!(fake.uids("Archive").len(), 1);
}

/// The target renews between two source folders: the first new IDs are
/// stale, so the result is unknown (#1024).
#[test]
fn move_target_uidvalidity() {
    let fake = server();
    let mail = provider(&fake);
    let a = add(&fake, "INBOX", 1);
    let b = add(&fake, "Archive", 2);
    // The second SELECT (Archive) renews Trash before the second move.
    fake.renew_at_select("Trash", 2);
    let r = mail.relocate(PASSWORD, &[&a, &b], Move::Trash);
    assert!(matches!(&r, Err(e) if e.write_result_unknown()), "{r:?}");
    assert_eq!(moves_sent(&fake), 2);
}

/// An answer that does not name each moved UID is unknown.
#[test]
fn move_partial_copyuid() {
    for mode in [
        Moving::NoCopyuid,
        Moving::PartialCopyuid,
        Moving::RefuseAfterOne,
        Moving::Close,
    ] {
        let fake = server();
        fake.set_moving(mode);
        let mail = provider(&fake);
        let a = add(&fake, "INBOX", 1);
        let b = add(&fake, "INBOX", 2);
        let r = mail.relocate(PASSWORD, &[&a, &b], Move::Archive);
        assert!(
            matches!(&r, Err(e) if e.write_result_unknown()),
            "{mode:?}: {r:?}"
        );
        assert_eq!(moves_sent(&fake), 1, "{mode:?}");
    }
}

/// Gmail is refused before the CONDSTORE check (issue #981).
#[test]
fn gmail_without_condstore_is_gmail_imap() {
    let fake = FakeImap::start(Caps {
        gmail: true,
        condstore: false,
        qresync: false,
        ..Caps::ALL
    });
    let mail = provider(&fake);
    assert_eq!(reason(mail.profile(PASSWORD)), "gmailImap");
}
