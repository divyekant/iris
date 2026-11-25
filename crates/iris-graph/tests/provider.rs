//! The Outlook provider against the fake Graph server: the calls that the
//! sync tests in `iris-sync` do not reach on their own.

use std::time::Duration;

use iris_fakegraph::FakeGraph;
use iris_graph::{Config, Graph, Outlook};
use iris_provider::{Error, Provider};
use iris_synth::Mailbox;

const TOKEN: &str = "test-token";

fn graph(fake: &FakeGraph) -> Graph {
    Graph::new(Config {
        base_url: fake.base_url(),
        backoff_start: Duration::from_millis(5),
        backoff_max: Duration::from_millis(50),
        ..Config::default()
    })
}

#[test]
fn the_master_categories_are_read_page_by_page() {
    let fake = FakeGraph::start(Mailbox::new(3, 10)).unwrap();
    let g = graph(&fake);
    let next = format!("{}/me/outlook/masterCategories?$skip=1", fake.base_url());
    let first =
        format!(r#"{{"value":[{{"id":"1","displayName":"One"}}],"@odata.nextLink":"{next}"}}"#);
    fake.answer_next(&[&first, r#"{"value":[{"id":"2","displayName":"Two"}]}"#]);
    assert_eq!(g.categories(TOKEN).unwrap(), vec!["One", "Two"]);
}

/// The list of all mail has each message one time, newest first across
/// folders (spec 1 section 4.2), also with small pages.
#[test]
fn all_mail_is_listed_newest_first_across_folders() {
    let mailbox = Mailbox::new(5, 150);
    let fake = FakeGraph::start(Mailbox::new(5, 150)).unwrap();
    fake.set_page_limit(17);
    let outlook = Outlook::new(graph(&fake));
    let mut ids = Vec::new();
    let mut page: Option<String> = None;
    loop {
        let p = outlook.messages(TOKEN, None, page.as_deref()).unwrap();
        ids.extend(p.messages.into_iter().map(|m| m.id));
        match p.next_page_token {
            Some(t) => page = Some(t),
            None => break,
        }
    }
    let mut want: Vec<usize> = (0..mailbox.len()).collect();
    want.sort_by_key(|&i| (std::cmp::Reverse(mailbox.truth(i).date), i));
    let want: Vec<String> = want.into_iter().map(FakeGraph::id_of).collect();
    assert_eq!(ids, want);
}

fn message_json(received: &str, created: &str) -> String {
    format!(
        r#"{{"id":"AAMk00000001","conversationId":"c","parentFolderId":"F-drafts",
        "categories":[],"isRead":true,"flag":{{"flagStatus":"notFlagged"}},"isDraft":true,
        "receivedDateTime":{received},"createdDateTime":{created},"internetMessageId":null}}"#
    )
}

/// A draft with no received time gets the time it was made, never 1970.
#[test]
fn a_draft_with_no_received_time_gets_its_created_time() {
    let fake = FakeGraph::start(Mailbox::new(3, 10)).unwrap();
    let outlook = Outlook::new(graph(&fake));
    fake.answer_next(&[&message_json("null", r#""2026-11-17T10:00:00Z""#)]);
    let m = outlook
        .raw(TOKEN, &FakeGraph::id_of(0), &mut Vec::new())
        .unwrap();
    assert_eq!(m.internal_date, 1_794_909_600_000);

    fake.answer_next(&[&message_json("null", "null")]);
    match outlook.raw(TOKEN, &FakeGraph::id_of(0), &mut Vec::new()) {
        Err(Error::Decode(_)) => {}
        other => panic!("a message with no time: {other:?}"),
    }
}

fn warm(fake: &FakeGraph) -> (Outlook, String, String) {
    let outlook = Outlook::new(graph(fake));
    let inbox: Vec<String> = fake
        .messages()
        .into_iter()
        .filter(|(_, f)| f == "F-inbox")
        .map(|(id, _)| id)
        .take(2)
        .collect();
    // The well-known folder IDs are read here, before the counted calls.
    outlook.minimal(TOKEN, &inbox[0]).unwrap();
    (outlook, inbox[0].clone(), inbox[1].clone())
}

/// A write that fails after another write of the call ran is an unknown
/// result; a refusal of the first write is definite and changes nothing.
#[test]
fn a_failure_after_a_write_of_the_call_is_unknown() {
    let fake = FakeGraph::start(Mailbox::new(3, 60)).unwrap();
    let (outlook, a, b) = warm(&fake);
    // Two reads, the first PATCH, then the second PATCH gets 400.
    fake.fail_next(&[(0, None), (0, None), (0, None), (400, None)]);
    let e = outlook
        .modify(TOKEN, &[&a, &b], &["STARRED"], &[])
        .unwrap_err();
    assert!(matches!(e, Error::Transport(_)), "{e}");
    assert!(e.write_result_unknown());
    assert!(outlook
        .minimal(TOKEN, &a)
        .unwrap()
        .label_ids
        .contains(&"STARRED".to_string()));

    // An unfollowed redirect of the first write is an unknown result too.
    fake.fail_next(&[(0, None), (307, None)]);
    let e = outlook.modify(TOKEN, &[&b], &["STARRED"], &[]).unwrap_err();
    assert!(e.write_result_unknown(), "{e}");
    fake.fail_next(&[(0, None), (307, None)]);
    assert!(outlook.trash(TOKEN, &b).unwrap_err().write_result_unknown());

    fake.fail_next(&[(0, None), (400, None)]);
    let e = outlook.modify(TOKEN, &[&b], &["STARRED"], &[]).unwrap_err();
    assert!(matches!(e, Error::Api { status: 400, .. }), "{e}");
    assert!(!e.write_result_unknown());
    assert!(!outlook
        .minimal(TOKEN, &b)
        .unwrap()
        .label_ids
        .contains(&"STARRED".to_string()));
}

/// A refusal of one message comes before the write of any message. A
/// change that is not a move works in a user folder.
#[test]
fn a_refusal_of_one_message_stops_the_writes_of_all() {
    let fake = FakeGraph::start(Mailbox::new(3, 60)).unwrap();
    let (outlook, a, _) = warm(&fake);
    let user = fake
        .messages()
        .into_iter()
        .find(|(_, f)| f == "F-old" || f == "F-receipts" || f == "F-projects")
        .map(|(id, _)| id)
        .expect("a message in a user folder");
    let before = fake.requests().len();
    let e = outlook
        .modify(TOKEN, &[&a, &user], &["UNREAD"], &["INBOX"])
        .unwrap_err();
    assert!(matches!(e, Error::Api { status: 400, .. }), "{e}");
    let writes = fake.requests()[before..]
        .iter()
        .filter(|r| !r.starts_with("GET"))
        .count();
    assert_eq!(writes, 0);
    // The read state, the flag and the categories are not moves: they
    // change in a user folder too (spec section 6).
    outlook
        .modify(TOKEN, &[&user], &["STARRED", "UNREAD"], &[])
        .unwrap();
    let l = outlook.minimal(TOKEN, &user).unwrap().label_ids;
    assert!(
        l.contains(&"STARRED".to_string()) && l.contains(&"UNREAD".to_string()),
        "{l:?}"
    );
    assert!(fake.messages()[&user].starts_with("F-") && fake.messages()[&user] != "F-inbox");
    let ids: Vec<&str> = std::iter::repeat_n(a.as_str(), iris_provider::BATCH_LIMIT + 1).collect();
    assert!(matches!(
        outlook.modify(TOKEN, &ids, &["UNREAD"], &[]),
        Err(Error::Api { status: 400, .. })
    ));
}

/// Another client changes the categories between the read of the plan
/// and the PATCH: Graph gives 412, the provider reads again, and the new
/// list keeps the change of the other client.
#[test]
fn a_category_conflict_reads_again_and_keeps_the_other_change() {
    let fake = FakeGraph::start(Mailbox::new(3, 60)).unwrap();
    let (outlook, a, _) = warm(&fake);
    // The read of the plan sees no category and an old etag.
    fake.answer_next(&[&format!(
        r#"{{"id":"{a}","@odata.etag":"W/\"old\"","conversationId":"c",
        "parentFolderId":"F-inbox","categories":[],"isRead":true,
        "flag":{{"flagStatus":"notFlagged"}},"isDraft":false,
        "receivedDateTime":"2026-11-17T10:00:00Z","internetMessageId":null}}"#
    )]);
    assert!(fake.set_categories(&a, &["Other client"]));
    outlook
        .modify(TOKEN, &[&a], &["category:Blue category"], &[])
        .unwrap();
    let l = outlook.minimal(TOKEN, &a).unwrap().label_ids;
    for c in ["category:Other client", "category:Blue category"] {
        assert!(l.contains(&c.to_string()), "{l:?}");
    }
    // A second conflict on the new read is the error of the call.
    let before = fake.requests().len();
    fake.answer_next(&[&format!(
        r#"{{"id":"{a}","@odata.etag":"W/\"old\"","conversationId":"c",
        "parentFolderId":"F-inbox","categories":[],"isRead":true,
        "flag":{{"flagStatus":"notFlagged"}},"isDraft":false,
        "receivedDateTime":"2026-11-17T10:00:00Z","internetMessageId":null}}"#
    )]);
    fake.fail_next(&[(0, None), (0, None), (0, None), (412, None)]);
    let e = outlook
        .modify(TOKEN, &[&a], &["category:Green"], &[])
        .unwrap_err();
    assert!(matches!(e, Error::Api { status: 412, .. }), "{e}");
    assert!(!e.write_result_unknown());
    assert_eq!(
        fake.requests().len() - before,
        4,
        "{:?}",
        &fake.requests()[before..]
    );
}

/// Another client moves the message to a user folder after the read of
/// the plan: the conditional `isRead` gets 412, the new read plans from
/// the user folder, and the call refuses the move before any write.
#[test]
fn a_move_by_another_client_stops_the_planned_move() {
    let fake = FakeGraph::start(Mailbox::new(3, 60)).unwrap();
    let (outlook, a, _) = warm(&fake);
    assert!(fake.set_read(&a, true));
    fake.answer_next(&[&format!(
        r#"{{"id":"{a}","@odata.etag":"W/\"old\"","conversationId":"c",
        "parentFolderId":"F-inbox","categories":[],"isRead":true,
        "flag":{{"flagStatus":"notFlagged"}},"isDraft":false,
        "receivedDateTime":"2026-11-17T10:00:00Z","internetMessageId":null}}"#
    )]);
    assert!(fake.move_message(&a, "F-old"));
    let e = outlook
        .modify(TOKEN, &[&a], &["UNREAD"], &["INBOX"])
        .unwrap_err();
    assert!(matches!(e, Error::Api { status: 400, .. }), "{e}");
    assert_eq!(fake.messages()[&a], "F-old");
    let l = outlook.minimal(TOKEN, &a).unwrap().label_ids;
    assert!(!l.contains(&"UNREAD".to_string()), "{l:?}");
}

/// Graph accepts a category PATCH and keeps another list (A6): the result
/// is unknown, and each later category write of the provider is refused
/// before any request. Other writes go on.
#[test]
fn a_kept_category_list_turns_off_category_writes() {
    let fake = FakeGraph::start(Mailbox::new(3, 60)).unwrap();
    let (outlook, a, b) = warm(&fake);
    let etag = fake.etag_of(&a).unwrap();
    // The read of the plan has no category; the PATCH answer keeps
    // another list.
    let kept = format!(
        r#"{{"id":"{a}","@odata.etag":{etag:?},"conversationId":"c",
        "parentFolderId":"F-inbox","categories":["Other"],"isRead":true,
        "flag":{{"flagStatus":"notFlagged"}},"isDraft":false,
        "receivedDateTime":"2026-11-17T10:00:00Z","internetMessageId":null}}"#
    );
    let n = fake.requests().len();
    let plan_read = format!(
        r#"{{"id":"{a}","@odata.etag":{etag:?},"conversationId":"c",
        "parentFolderId":"F-inbox","categories":[],"isRead":true,
        "flag":{{"flagStatus":"notFlagged"}},"isDraft":false,
        "receivedDateTime":"2026-11-17T10:00:00Z","internetMessageId":null}}"#
    );
    fake.answer_next(&[&plan_read, &kept]);
    let e = outlook
        .modify(TOKEN, &[&a], &["category:Blue"], &[])
        .unwrap_err();
    assert!(e.write_result_unknown(), "{e}");
    let after = fake.requests().len();
    assert!(after > n);
    for (add, remove) in [
        (&["category:Blue"][..], &[][..]),
        (&[][..], &["category:Other"][..]),
    ] {
        let e = outlook.modify(TOKEN, &[&b], add, remove).unwrap_err();
        assert!(matches!(e, Error::Api { status: 400, .. }), "{e}");
        assert!(!e.write_result_unknown());
    }
    assert_eq!(fake.requests().len(), after, "no request after the switch");
    outlook.modify(TOKEN, &[&b], &["STARRED"], &[]).unwrap();
}

/// The draft calls of the provider: a draft above the limit has no raw
/// bytes, a message that is not a draft is gone, and a send with no
/// `Message-ID` is refused before any request.
#[test]
fn the_draft_calls_keep_their_limits() {
    let fake = FakeGraph::start(Mailbox::new(3, 60)).unwrap();
    let (outlook, a, _) = warm(&fake);
    let mut big =
        b"From: <owner@example.com>\r\nMessage-ID: <big@example.com>\r\nSubject: x\r\n\r\n"
            .to_vec();
    big.resize(iris_provider::DRAFT_LIMIT + 10, b'a');
    // A draft that the person made in Outlook: Graph refuses a create of
    // this size.
    let id = fake.add_message(&big, "F-drafts");
    let d = outlook.drafts_with_id(TOKEN, "big@example.com").unwrap()[0].clone();
    assert_eq!(d.id, id);
    assert_eq!(outlook.draft_raw(TOKEN, &d.id).unwrap().raw, None);
    assert_eq!(
        outlook.drafts_with_id(TOKEN, "big@example.com").unwrap(),
        vec![d.clone()]
    );
    assert_eq!(
        outlook.drafts_with_id(TOKEN, "<big@example.com>").unwrap(),
        vec![d]
    );
    assert!(matches!(outlook.draft_raw(TOKEN, &a), Err(Error::NotFound)));
    let before = fake.requests().len();
    let e = outlook
        .send_draft(
            TOKEN,
            &a,
            b"From: <owner@example.com>\r\nSubject: x\r\n\r\nHi.\r\n",
            None,
        )
        .unwrap_err();
    assert!(matches!(e, Error::Api { status: 400, .. }), "{e}");
    assert_eq!(fake.requests().len(), before);
}

/// One full `changes` run from `cursor`. `after` runs after each call with
/// the number of calls so far. Gives the records and the new cursor.
fn changes_run(
    outlook: &Outlook,
    cursor: &str,
    mut after: impl FnMut(usize),
) -> (Vec<iris_provider::Change>, String) {
    let mut records = Vec::new();
    let mut page: Option<String> = None;
    let mut n = 0;
    loop {
        let p = outlook.changes(TOKEN, cursor, page.as_deref()).unwrap();
        records.extend(p.history.into_iter().flat_map(|h| h.changes));
        n += 1;
        after(n);
        match p.next_page_token {
            Some(t) => page = Some(t),
            None => return (records, p.cursor),
        }
    }
}

/// The labels of `id` after all records: the sync applies them in order,
/// and the last `Added` sets the labels (#665).
fn last_labels(records: &[iris_provider::Change], id: &str) -> Option<Vec<String>> {
    records.iter().rev().find_map(|r| match r {
        iris_provider::Change::Added(m) if m.id == id => Some(m.label_ids.clone()),
        _ => None,
    })
}

/// A message leaves folder A, a later round lists it in folder B, then it
/// moves to folder C whose round ended. The read by ID of the `Removed` of A
/// runs after the last round, so the sync ends with the labels of C (#665).
#[test]
fn a_move_during_the_sync_ends_with_the_folder_after_the_move() {
    let fake = FakeGraph::start(Mailbox::new(3, 60)).unwrap();
    let outlook = Outlook::new(graph(&fake));
    let order: Vec<String> = graph(&fake)
        .folders(TOKEN)
        .unwrap()
        .into_iter()
        .map(|f| f.id)
        .collect();
    let (a, b, c) = (order[1].clone(), order[2].clone(), order[0].clone());
    let start = outlook.profile(TOKEN).unwrap().cursor;
    let (_, cursor) = changes_run(&outlook, &start, |_| {});
    let id = fake
        .messages()
        .into_iter()
        .find(|(_, f)| *f == a)
        .map(|(id, _)| id)
        .expect("a message in folder A");
    assert!(fake.move_message(&id, &b));
    // Each folder gives one page: call 3 is the round of B.
    let (records, _) = changes_run(&outlook, &cursor, |n| {
        if n == 3 {
            assert!(fake.move_message(&id, &c));
        }
    });
    let want = outlook.minimal(TOKEN, &id).unwrap().label_ids;
    assert_eq!(last_labels(&records, &id), Some(want));
}

/// A message that leaves a folder and is deleted gives `Deleted` from the
/// read after the last round, and a message that only moved gives its new
/// folder.
#[test]
fn the_reads_after_the_last_round_give_moves_and_deletes() {
    let fake = FakeGraph::start(Mailbox::new(3, 60)).unwrap();
    fake.set_page_limit(5);
    let outlook = Outlook::new(graph(&fake));
    let start = outlook.profile(TOKEN).unwrap().cursor;
    let (_, cursor) = changes_run(&outlook, &start, |_| {});
    let inbox: Vec<String> = fake
        .messages()
        .into_iter()
        .filter(|(_, f)| f == "F-inbox")
        .map(|(id, _)| id)
        .collect();
    let (moved, gone) = (&inbox[0], &inbox[1]);
    assert!(fake.move_message(moved, FakeGraph::folder_id("Old")));
    assert!(fake.delete_message(gone));
    let (records, _) = changes_run(&outlook, &cursor, |_| {});
    let want = outlook.minimal(TOKEN, moved).unwrap().label_ids;
    assert_eq!(last_labels(&records, moved), Some(want));
    assert!(records
        .iter()
        .any(|r| matches!(r, iris_provider::Change::Deleted(m) if &m.id == gone)));
    assert_eq!(last_labels(&records, gone), None);
}

/// Past 1,000 removed IDs in one sync, a `Removed` gets its read at once.
/// Each moved message still ends with its folder now.
#[test]
fn more_removals_than_the_page_token_keeps_still_give_each_move() {
    let fake = FakeGraph::start(Mailbox::new(3, 1_300)).unwrap();
    let outlook = Outlook::new(graph(&fake));
    let start = outlook.profile(TOKEN).unwrap().cursor;
    let (_, cursor) = changes_run(&outlook, &start, |_| {});
    let old = FakeGraph::folder_id("Old");
    let moved: Vec<String> = fake
        .messages()
        .into_iter()
        .filter(|(_, f)| f != old)
        .map(|(id, _)| id)
        .collect();
    assert!(moved.len() > 1_000, "{}", moved.len());
    for id in &moved {
        assert!(fake.move_message(id, old));
    }
    let (records, _) = changes_run(&outlook, &cursor, |_| {});
    for id in &moved {
        let want = outlook.minimal(TOKEN, id).unwrap().label_ids;
        assert!(want.contains(&format!("folder:{old}")), "{want:?}");
        assert_eq!(last_labels(&records, id), Some(want), "{id}");
    }
}

/// Issue #976: a list of a label other than `SENT` never asks the sent
/// source, so a failed source cannot break an INBOX list.
#[test]
fn a_label_list_other_than_sent_does_not_ask_the_sent_source() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    let fake = FakeGraph::start(Mailbox::new(5, 40)).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let outlook = Outlook::new(graph(&fake)).with_sent_source(move |_| {
        seen.fetch_add(1, Ordering::SeqCst);
        Err("disk I/O error".into())
    });
    let mut listed = 0;
    let mut page: Option<String> = None;
    loop {
        let p = outlook
            .messages(TOKEN, Some("INBOX"), page.as_deref())
            .unwrap();
        listed += p.messages.len();
        match p.next_page_token {
            Some(t) => page = Some(t),
            None => break,
        }
    }
    assert!(listed > 0, "the INBOX list is empty");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    // The counter works: a `SENT` list asks the source and gets its error.
    let err = outlook.messages(TOKEN, Some("SENT"), None);
    assert!(err.is_err());
    assert!(calls.load(Ordering::SeqCst) > 0);
}
