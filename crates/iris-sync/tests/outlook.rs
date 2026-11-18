//! The sync and the verify of an Outlook account through the Graph provider
//! (`iris_graph::Outlook`) against the fake Graph server (card R6-14,
//! spec `docs/specs/2026-11-17-outlook-provider.md`). No network.

use std::collections::BTreeMap;
use std::time::Duration;

use iris_fakegraph::FakeGraph;
use iris_graph::{Config, Graph, Outlook};
use iris_store::Store;
use iris_sync::{sync, verify};
use iris_synth::Mailbox;

mod common;
use common::{count, data_dir, mirror_state, OWNER, TOKEN};

fn outlook(fake: &FakeGraph) -> Outlook {
    Outlook::new(Graph::new(Config {
        base_url: fake.base_url(),
        backoff_start: Duration::from_millis(5),
        backoff_max: Duration::from_millis(50),
        ..Config::default()
    }))
}

fn synced(name: &str, seed: u64, n: usize) -> (FakeGraph, Store, i64) {
    let fake = FakeGraph::start(Mailbox::new(seed, n)).unwrap();
    let store = Store::open(&data_dir(name)).unwrap();
    let account = store.add_account("outlook", OWNER).unwrap();
    sync(&store, &outlook(&fake), account, TOKEN).unwrap();
    (fake, store, account)
}

/// The folder label that the mirror must give a message of `folder` (spec
/// section 6). The fake makes each message of Drafts a draft.
fn folder_label(folder: &str) -> String {
    match folder {
        "F-inbox" => "INBOX".into(),
        "F-deleted" => "TRASH".into(),
        "F-junk" => "SPAM".into(),
        "F-drafts" => "DRAFT".into(),
        f => format!("folder:{f}"),
    }
}

/// Zero differences, the same messages as the fake, and the folder label of
/// each message from the fake itself, not from the provider.
fn assert_same(fake: &FakeGraph, store: &Store, account: i64) {
    assert_same_but(fake, store, account, &[]);
}

/// [`assert_same`], with the folder label of each message of `but` given:
/// a received message that moved into Drafts is not a draft.
fn assert_same_but(fake: &FakeGraph, store: &Store, account: i64, but: &[(&str, &str)]) {
    let r = verify(store, &outlook(fake), account, TOKEN).unwrap();
    assert_eq!(r.differences, vec![], "verify");
    assert!(!r.resync);
    let truth = fake.messages();
    let mirror: BTreeMap<String, Vec<String>> = mirror_state(store).into_iter().collect();
    assert_eq!(
        mirror.keys().collect::<Vec<_>>(),
        truth.keys().collect::<Vec<_>>(),
        "the messages of the mirror"
    );
    for (id, folder) in &truth {
        let labels = &mirror[id];
        let folders: Vec<&String> = labels
            .iter()
            .filter(|l| {
                l.starts_with("folder:")
                    || ["INBOX", "TRASH", "SPAM", "DRAFT"].contains(&l.as_str())
            })
            .collect();
        let want = match but.iter().find(|(m, _)| m == id) {
            Some((_, label)) => label.to_string(),
            None => folder_label(folder),
        };
        assert_eq!(folders, vec![&want], "the folder of {id}");
    }
}

fn label_name(store: &Store, id: &str) -> Option<String> {
    store
        .read(|c| {
            Ok(
                c.query_row("SELECT name FROM label WHERE provider_id = ?1", [id], |r| {
                    r.get(0)
                })
                .ok(),
            )
        })
        .unwrap()
}

fn row(store: &Store, id: &str) -> (i64, Option<i64>) {
    store
        .read(|c| {
            Ok(c.query_row(
                "SELECT id, gone_at FROM message WHERE provider_id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap()
}

/// The first message of the fake in `folder`.
fn one_in(fake: &FakeGraph, folder: &str, skip: usize) -> String {
    fake.messages()
        .into_iter()
        .filter(|(_, f)| f == folder)
        .nth(skip)
        .map(|(id, _)| id)
        .expect("a message in the folder")
}

#[test]
fn an_outlook_mailbox_syncs_with_no_difference() {
    let (fake, store, account) = synced("o-clean", 31, 200);
    assert_same(&fake, &store, account);
    assert_eq!(
        label_name(&store, "folder:F-old").as_deref(),
        Some("Folders/Receipts/Projects/Old")
    );
    assert_eq!(
        label_name(&store, "folder:F-hidden").as_deref(),
        Some("Folders/Receipts/Hidden"),
        "a hidden child folder"
    );
    assert_eq!(
        label_name(&store, "folder:F-search"),
        None,
        "a search folder"
    );
    assert_eq!(
        label_name(&store, "folder:F-sent").as_deref(),
        Some("Folders/Sent Items"),
        "no SENT from the folder"
    );
    assert_eq!(
        label_name(&store, "category:Receipts").as_deref(),
        Some("Receipts")
    );
    assert_eq!(
        count(
            &store,
            "SELECT count(*) FROM label WHERE provider_id = 'SENT'"
        ),
        0
    );
}

#[test]
fn a_move_a_delete_and_changes_reach_the_mirror() {
    let (fake, store, account) = synced("o-changes", 32, 200);
    let moved = one_in(&fake, "F-inbox", 0);
    let back = one_in(&fake, "F-archive", 0);
    let deleted = one_in(&fake, "F-inbox", 1);
    let tagged = one_in(&fake, "F-inbox", 2);
    let to_drafts = one_in(&fake, "F-inbox", 3);
    let to_sent = one_in(&fake, "F-inbox", 4);
    let (moved_row, _) = row(&store, &moved);
    let (back_row, _) = row(&store, &back);
    assert!(fake.move_message(&moved, "F-archive"));
    // The round of Inbox comes first: only the Removed of Archive shows this move.
    assert!(fake.move_message(&back, "F-inbox"));
    assert!(fake.delete_message(&deleted));
    assert!(fake.move_message(&to_drafts, "F-drafts"));
    assert!(fake.move_message(&to_sent, "F-sent"));
    assert!(fake.set_read(&tagged, false));
    assert!(fake.set_categories(&tagged, &["Travel", "travel", "Inbox"]));
    let new = fake.add_message(
        b"Message-ID: <new@example.com>\r\nFrom: a@example.com\r\nSubject: hi\r\n\r\nhello\r\n",
        "F-inbox",
    );

    let s = sync(&store, &outlook(&fake), account, TOKEN).unwrap();
    assert!(!s.resync);
    assert_eq!(s.gone, 1);
    assert_eq!(
        row(&store, &moved),
        (moved_row, None),
        "a move keeps the row"
    );
    assert_eq!(row(&store, &back), (back_row, None));
    assert!(row(&store, &deleted).1.is_some(), "the delete");
    assert!(row(&store, &new).1.is_none(), "the new message");
    let hex = |s: &str| s.bytes().map(|b| format!("{b:02x}")).collect::<String>();
    assert_eq!(
        label_name(&store, "category:Travel"),
        Some(format!("Categories/Travel #{}", hex("Travel")))
    );
    assert_eq!(
        label_name(&store, "category:Inbox").as_deref(),
        Some("Categories/Inbox"),
        "a category with a system name"
    );
    // Received mail moved into Drafts or Sent Items is not proof of the owner.
    assert_same_but(&fake, &store, account, &[(&to_drafts, "folder:F-drafts")]);
    let labels: BTreeMap<String, Vec<String>> = mirror_state(&store).into_iter().collect();
    assert!(labels[&to_sent].contains(&"folder:F-sent".to_string()));
    assert!(!labels[&to_drafts].contains(&"DRAFT".to_string()));
}

#[test]
fn an_expired_link_starts_a_full_resync() {
    let (fake, store, account) = synced("o-expired", 33, 200);
    let moved = one_in(&fake, "F-inbox", 0);
    let deleted = one_in(&fake, "F-inbox", 1);
    assert!(fake.move_message(&moved, "F-old"));
    assert!(fake.delete_message(&deleted));
    // The links of the first sync hold an older sequence number.
    fake.expire_delta_before(fake.seq());

    let s = sync(&store, &outlook(&fake), account, TOKEN).unwrap();
    assert!(s.resync, "a full resync");
    assert!(row(&store, &deleted).1.is_some(), "the delete");
    assert!(row(&store, &moved).1.is_none(), "the move");
    assert_same(&fake, &store, account);
}

#[test]
fn a_second_sync_with_no_change_changes_nothing() {
    let (fake, store, account) = synced("o-quiet", 34, 120);
    let before = mirror_state(&store);
    let s = sync(&store, &outlook(&fake), account, TOKEN).unwrap();
    assert_eq!((s.fetched, s.gone, s.failed, s.resync), (0, 0, 0, false));
    assert_eq!(mirror_state(&store), before);
}
