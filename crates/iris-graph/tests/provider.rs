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
