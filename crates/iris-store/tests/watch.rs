//! Watches (card R5-07): one notice for the first new message that matches.

use std::fs;
use std::path::PathBuf;

use iris_store::{Body, Error, Header, NewMessage, Store, SINCE_MARGIN_MS, WAIT_MAX};

struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> TempDir {
        let dir = std::env::temp_dir().join(format!("iris-watch-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const NOW: i64 = 1_794_700_800_000; // 2026-11-15

fn msg(id: &str, date: i64, labels: &[&str], subject: &str, body: bool) -> NewMessage {
    NewMessage {
        provider_id: id.into(),
        thread_provider_id: format!("t-{id}"),
        internal_date: Some(date),
        labels: labels.iter().map(|l| l.to_string()).collect(),
        header: Header {
            from_addr: Some(format!("{id}@example.com")),
            from_name: Some("Ana Lee".into()),
            subject: Some(subject.into()),
            ..Header::default()
        },
        body: body.then(|| Body {
            subject: subject.into(),
            sender: format!("Ana Lee {id}@example.com"),
            text: "The text.".into(),
        }),
        ..NewMessage::default()
    }
}

fn setup(dir: &TempDir) -> (Store, i64) {
    let store = Store::open(&dir.0).unwrap();
    let acc = store.add_account("gmail", "owner@example.com").unwrap();
    store
        .put_labels(
            acc,
            &[
                ("INBOX", "INBOX", "system"),
                ("SENT", "SENT", "system"),
                ("DRAFT", "DRAFT", "system"),
                ("UNSENT", "UNSENT", "system"),
                ("SENT_COPY", "SENT_COPY", "system"),
                ("SPAM", "SPAM", "system"),
            ],
        )
        .unwrap();
    (store, acc)
}

fn put(store: &Store, acc: i64, m: NewMessage) -> i64 {
    store.put_messages(acc, &[m]).unwrap()[0]
}

#[test]
fn a_watch_gives_one_notice_for_the_first_new_match() {
    let dir = TempDir::new("one");
    let (store, acc) = setup(&dir);
    let old = put(
        &store,
        acc,
        msg("old", NOW - 60_000, &["INBOX"], "Visa decision", true),
    );
    let w = store.add_watch("  visa  ", NOW).unwrap();
    assert_eq!(
        store.check_watches(NOW).unwrap(),
        [],
        "mail from before the watch"
    );

    put(
        &store,
        acc,
        msg("other", NOW + 1, &["INBOX"], "Lunch", true),
    );
    assert_eq!(store.check_watches(NOW + 2).unwrap(), []);

    let first = put(
        &store,
        acc,
        msg("v1", NOW + 3, &["INBOX"], "Your visa", true),
    );
    let second = put(
        &store,
        acc,
        msg("v2", NOW + 4, &["INBOX"], "Visa again", true),
    );
    assert!(old < first && first < second);
    assert_eq!(store.check_watches(NOW + 5).unwrap(), [(w, first)]);
    // One notice: the watch does not match again.
    assert_eq!(store.check_watches(NOW + 6).unwrap(), []);
    let got = store.watch(w).unwrap().unwrap();
    assert_eq!(got.text, "visa");
    assert_eq!((got.message, got.arrived_at), (Some(first), Some(NOW + 5)));

    assert!(store.remove_watch(w).unwrap());
    assert!(!store.remove_watch(w).unwrap());
    assert_eq!(store.watches().unwrap(), []);
}

#[test]
fn a_watch_skips_mail_that_is_not_a_safe_new_arrival() {
    let dir = TempDir::new("skip");
    let (store, acc) = setup(&dir);
    let w = store.add_watch("from:ana", NOW).unwrap();
    // Each of these matches the search, but none is a safe new arrival.
    let pending = put(&store, acc, msg("a", NOW, &["INBOX"], "x", false));
    put(
        &store,
        acc,
        msg("b", NOW - SINCE_MARGIN_MS - 1, &["INBOX"], "x", true),
    );
    put(&store, acc, msg("c", NOW, &["SENT"], "x", true));
    put(&store, acc, msg("d", NOW, &["DRAFT"], "x", true));
    // The IMAP labels of a draft and of a copy in a Sent folder.
    put(&store, acc, msg("d2", NOW, &["UNSENT"], "x", true));
    put(
        &store,
        acc,
        msg("d3", NOW, &["INBOX", "SENT_COPY"], "x", true),
    );
    // The label SPAM before the check (decision D33).
    put(&store, acc, msg("s", NOW, &["INBOX", "SPAM"], "x", true));
    let marked = put(&store, acc, msg("e", NOW, &["INBOX"], "x", true));
    let _hostile_sender = put(&store, acc, msg("f", NOW, &["INBOX"], "x", true));
    let gone = put(&store, acc, msg("g", NOW, &["INBOX"], "x", true));
    store
        .write(|tx| {
            tx.execute(
                "UPDATE message SET hostile = '[\"spoof\"]' WHERE id = ?1",
                [marked],
            )?;
            tx.execute(
                "INSERT INTO sender(address, kind) VALUES ('F@example.com', 'hostile')",
                [],
            )?;
            tx.execute("UPDATE message SET gone_at = 1 WHERE id = ?1", [gone])?;
            Ok(())
        })
        .unwrap();
    assert_eq!(store.check_watches(NOW).unwrap(), []);

    // The clean text of the first message comes with its hostile check.
    store
        .put_body(
            pending,
            &Body {
                subject: "x".into(),
                sender: "Ana Lee a@example.com".into(),
                text: "t".into(),
            },
        )
        .unwrap();
    // Mail just before the watch that was not in the mirror can match.
    assert_eq!(store.check_watches(NOW).unwrap(), [(w, pending)]);
}

/// A message that gets the IMAP `\\Draft` flag (the label `UNSENT`) after
/// its body was saved gives no arrival notice (IMAP spec section 4).
#[test]
fn a_draft_flag_after_the_body_gives_no_notice() {
    let dir = TempDir::new("flag");
    let (store, acc) = setup(&dir);
    let w = store.add_watch("from:ana", NOW).unwrap();
    put(&store, acc, msg("a", NOW, &["INBOX"], "x", true));
    put(&store, acc, msg("a", NOW, &["INBOX", "UNSENT"], "x", true));
    assert_eq!(store.check_watches(NOW).unwrap(), []);
    assert_eq!(store.watch(w).unwrap().unwrap().message, None);
}

#[test]
fn a_watch_needs_a_valid_text_and_a_free_place() {
    let dir = TempDir::new("add");
    let (store, _) = setup(&dir);
    for bad in ["", "   ", "from:", "form:email", &"x".repeat(501)] {
        assert!(store.add_watch(bad, NOW).is_err(), "{bad:?}");
    }
    assert!(store.add_watch(&"x".repeat(500), NOW).is_ok());
    for _ in 1..WAIT_MAX {
        store.add_watch("visa", NOW).unwrap();
    }
    assert!(matches!(store.add_watch("visa", NOW), Err(Error::Watch(_))));
    assert_eq!(store.watches().unwrap().len() as i64, WAIT_MAX);
}

/// The invariant: a watch matches at most one time, and its message is the
/// lowest new row ID that the same search finds and that is a safe arrival.
/// Random mailboxes and random check times.
#[test]
fn a_watch_matches_the_first_safe_arrival_of_the_search() {
    let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = |n: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % n
    };
    for round in 0..40 {
        let dir = TempDir::new(&format!("prop-{round}"));
        let (store, acc) = setup(&dir);
        let words = ["visa", "lunch", "invoice"];
        let pre = next(4);
        for i in 0..pre {
            put(
                &store,
                acc,
                msg(
                    &format!("p{i}"),
                    NOW,
                    &["INBOX"],
                    words[next(3) as usize],
                    true,
                ),
            );
        }
        let text = words[next(3) as usize];
        let w = store.add_watch(text, NOW).unwrap();
        let mut safe = Vec::new();
        let mut fired = None;
        for i in 0..next(12) {
            let subject = words[next(3) as usize];
            let label = ["INBOX", "SENT", "DRAFT"][next(3) as usize];
            let body = next(4) != 0;
            let date = NOW - SINCE_MARGIN_MS + next(3) as i64 * SINCE_MARGIN_MS - 1;
            let id = put(
                &store,
                acc,
                msg(&format!("n{i}"), date, &[label], subject, body),
            );
            if subject == text && label == "INBOX" && body && date >= NOW - SINCE_MARGIN_MS {
                safe.push(id);
            }
            if next(3) == 0 {
                for (got_w, m) in store.check_watches(NOW).unwrap() {
                    assert_eq!(got_w, w);
                    assert!(fired.is_none(), "round {round}: a second notice");
                    fired = Some(m);
                }
                assert_eq!(fired, safe.first().copied(), "round {round}");
            }
        }
        for (_, m) in store.check_watches(NOW).unwrap() {
            assert!(fired.is_none(), "round {round}: a second notice");
            fired = Some(m);
        }
        assert_eq!(fired, safe.first().copied(), "round {round}");
        if let Some(m) = fired {
            assert!(store.search(text, 200).unwrap().contains(&m));
        }
    }
}

/// The invariant: only `remove_watch` ends a watch. The cleanup of a gone
/// message keeps the notice of a watch that matched it.
#[test]
fn the_cleanup_of_the_gone_message_keeps_the_notice() {
    let dir = TempDir::new("clean");
    let (store, acc) = setup(&dir);
    let w = store.add_watch("visa", NOW).unwrap();
    let m = put(&store, acc, msg("v", NOW, &["INBOX"], "Visa", true));
    assert_eq!(store.check_watches(NOW).unwrap(), [(w, m)]);
    store
        .write(|tx| Ok(tx.execute("UPDATE message SET gone_at = 1 WHERE id = ?1", [m])?))
        .unwrap();
    store.clean_gone(NOW).unwrap();
    let gone: i64 = store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM message", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(gone, 0, "the cleanup deleted the message row");
    let got = store.watch(w).unwrap().unwrap();
    assert_eq!((got.message, got.arrived_at), (Some(m), Some(NOW)));
    assert_eq!(store.check_watches(NOW).unwrap(), [], "no second notice");
}

#[test]
fn a_watch_waits_for_the_link_check() {
    let dir = TempDir::new("link-check");
    let (store, acc) = setup(&dir);
    let w = store.add_watch("from:ana", NOW).unwrap();
    let id = put(&store, acc, msg("a", NOW, &["INBOX"], "x", true));
    store
        .write(|tx| {
            tx.execute(
                "INSERT OR IGNORE INTO link_check(message_id) VALUES (?1)",
                [id],
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(store.check_watches(NOW).unwrap(), []);
    store
        .write(|tx| {
            tx.execute("DELETE FROM link_check WHERE message_id = ?1", [id])?;
            Ok(())
        })
        .unwrap();
    assert_eq!(store.check_watches(NOW).unwrap(), [(w, id)]);
}

/// Decision D33 (issue 257): a match that becomes hostile mail after the
/// check gives no notice. The watch waits again, and the next safe arrival
/// matches. Each later sign of hostile mail counts: a mark, the label
/// `SPAM`, a hostile sender.
#[test]
fn a_match_that_becomes_hostile_gives_no_notice() {
    let dir = TempDir::new("later-mark");
    let (store, acc) = setup(&dir);
    let cases: [(&str, &str); 3] = [
        (
            "mark",
            "UPDATE message SET hostile = '[\"phishing\"]' WHERE id = ?1",
        ),
        (
            "spam",
            "INSERT INTO message_label(message_id, label_id)
               SELECT ?1, id FROM label WHERE provider_id = 'SPAM'",
        ),
        (
            "sender",
            "INSERT INTO sender(address, kind, decided_by)
               SELECT from_addr, 'hostile', 'engine' FROM header WHERE message_id = ?1",
        ),
    ];
    for (i, (name, sql)) in cases.iter().enumerate() {
        let t = NOW + 10 * i as i64;
        let w = store.add_watch(&format!("visa{i}"), t).unwrap();
        let bad = put(
            &store,
            acc,
            msg(&format!("b{i}"), t, &["INBOX"], &format!("visa{i}"), true),
        );
        assert_eq!(store.check_watches(t).unwrap(), [(w, bad)], "{name}");
        store.write(|tx| Ok(tx.execute(sql, [bad])?)).unwrap();
        // The readers show no notice now, also before the next check.
        let got = store.watch(w).unwrap().unwrap();
        assert_eq!((got.message, got.arrived_at), (None, None), "{name}");
        let listed = store.watches().unwrap();
        let row = listed.iter().find(|x| x.id == w).unwrap();
        assert_eq!(row.message, None, "{name}");
        assert_eq!(store.check_watches(t + 1).unwrap(), [], "{name}");

        let good = put(
            &store,
            acc,
            msg(
                &format!("g{i}"),
                t + 2,
                &["INBOX"],
                &format!("visa{i}"),
                true,
            ),
        );
        assert_eq!(store.check_watches(t + 3).unwrap(), [(w, good)], "{name}");
        let got = store.watch(w).unwrap().unwrap();
        assert_eq!((got.message, got.arrived_at), (Some(good), Some(t + 3)));
    }
}

/// Decision D33 with the content cleanup: the cleanup deletes the header
/// (the sender evidence) and then the message row (the marks and labels) of
/// gone mail. A notice or a watch match on hostile mail must stay hidden
/// after that; a safe one stays as it is.
#[test]
fn the_cleanup_keeps_a_hostile_match_hidden() {
    let dir = TempDir::new("clean-hostile");
    let (store, acc) = setup(&dir);
    let cases = [
        (
            "mark",
            "UPDATE message SET hostile = '[\"phishing\"]' WHERE id = ?1",
        ),
        (
            "sender",
            "INSERT INTO sender(address, kind, decided_by)
               SELECT from_addr, 'hostile', 'engine' FROM header WHERE message_id = ?1",
        ),
        ("safe", "UPDATE message SET hostile = hostile WHERE id = ?1"),
    ];
    let notice = |id: i64| {
        store
            .write(|tx| {
                Ok(tx.execute(
                    "INSERT INTO notice(message_id, rule, rule_name, policy_version, title, text, created_at)
                     VALUES (?1, 1, 'r', 1, 't', 'x', 0)",
                    [id],
                )?)
            })
            .unwrap();
    };
    let open = |id: i64| -> bool {
        store
            .read(|c| {
                Ok(c.query_row(
                    "SELECT done_at IS NULL FROM notice WHERE message_id = ?1",
                    [id],
                    |r| r.get(0),
                )?)
            })
            .unwrap()
    };
    let mut rows = Vec::new();
    for (i, (name, sql)) in cases.iter().enumerate() {
        let t = NOW + 10 * i as i64;
        let w = store.add_watch(&format!("visa{i}"), t).unwrap();
        let m = put(
            &store,
            acc,
            msg(&format!("c{i}"), t, &["INBOX"], &format!("visa{i}"), true),
        );
        assert_eq!(store.check_watches(t).unwrap(), [(w, m)], "{name}");
        notice(m);
        store.write(|tx| Ok(tx.execute(sql, [m])?)).unwrap();
        store
            .write(|tx| Ok(tx.execute("UPDATE message SET gone_at = 1 WHERE id = ?1", [m])?))
            .unwrap();
        rows.push((*name, w, m));
    }
    store.clean_gone(NOW).unwrap();
    let left: i64 = store
        .read(|c| Ok(c.query_row("SELECT count(*) FROM message", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(left, 0, "the cleanup deleted the evidence");
    for (name, w, m) in rows {
        let got = store.watch(w).unwrap().unwrap();
        let safe = name == "safe";
        assert_eq!(got.message.is_some(), safe, "{name}");
        assert_eq!(open(m), safe, "{name}");
    }
}

/// A stored match follows the same rule as a new arrival (issue #931): a
/// match that gets the label `SENT` later is not mail that you got, so the
/// watch waits again, also after the content cleanup of the gone message.
#[test]
fn a_match_that_gets_sent_later_waits_again() {
    let dir = TempDir::new("sent-later");
    let (store, acc) = setup(&dir);
    let w = store.add_watch("visa", NOW).unwrap();
    let m = put(&store, acc, msg("a", NOW, &["INBOX"], "visa", true));
    assert_eq!(store.check_watches(NOW).unwrap(), [(w, m)]);
    put(&store, acc, msg("a", NOW, &["INBOX", "SENT"], "visa", true));
    assert_eq!(store.watch(w).unwrap().unwrap().message, None);
    let later = put(&store, acc, msg("b", NOW, &["INBOX"], "visa", true));
    assert_eq!(store.check_watches(NOW + 1).unwrap(), [(w, later)]);
    // The cleanup of the gone match keeps the watch waiting.
    let w2 = store.add_watch("lunch", NOW).unwrap();
    let m2 = put(&store, acc, msg("c", NOW, &["INBOX"], "lunch", true));
    assert_eq!(store.check_watches(NOW).unwrap(), [(w2, m2)]);
    put(
        &store,
        acc,
        msg("c", NOW, &["INBOX", "SENT"], "lunch", true),
    );
    store
        .write(|tx| Ok(tx.execute("UPDATE message SET gone_at = 1 WHERE id = ?1", [m2])?))
        .unwrap();
    store.clean_gone(NOW).unwrap();
    assert_eq!(store.watch(w2).unwrap().unwrap().message, None);
}
