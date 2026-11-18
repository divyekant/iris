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
