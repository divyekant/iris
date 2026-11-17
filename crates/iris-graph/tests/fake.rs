//! The client against the fake Graph server (spec acceptance criteria 1 to 5).

use std::collections::BTreeMap;
use std::time::Duration;

use iris_fakegraph::FakeGraph;
use iris_graph::{Change, Config, Error, Graph, Next};
use iris_synth::Mailbox;

const TOKEN: &str = "test-token";

fn setup(count: usize) -> (FakeGraph, Graph, Mailbox) {
    let fake = FakeGraph::start(Mailbox::new(7, count)).unwrap();
    let graph = Graph::new(Config {
        base_url: fake.base_url(),
        backoff_start: Duration::from_millis(5),
        backoff_max: Duration::from_millis(50),
        ..Config::default()
    });
    (fake, graph, Mailbox::new(7, count))
}

/// One full round of a folder from `link`: the state of each message that
/// the round names (true: in the folder), and the new delta link.
fn round(graph: &Graph, folder: &str, link: Option<String>) -> (BTreeMap<String, bool>, String) {
    let mut seen = BTreeMap::new();
    let mut link = link;
    loop {
        let page = graph.delta(TOKEN, folder, link.as_deref()).unwrap();
        for c in page.changes {
            match c {
                Change::Message(m) => {
                    assert_eq!(m.parent_folder_id, folder);
                    seen.insert(m.id, true)
                }
                Change::Removed(id) => seen.insert(id, false),
            };
        }
        match page.next {
            Next::More(l) => link = Some(l),
            Next::Done(l) => return (seen, l),
        }
    }
}

#[test]
fn a_full_sync_finds_each_folder_and_each_message() {
    let (fake, graph, mailbox) = setup(300);
    fake.set_page_limit(17);
    let folders = graph.folders(TOKEN).unwrap();
    let names: Vec<&str> = folders.iter().map(|f| f.display_name.as_str()).collect();
    for want in ["Inbox", "Receipts", "Projects", "Old", "Archive"] {
        assert!(names.contains(&want), "{want} in {names:?}");
    }
    let inbox = folders.iter().find(|f| f.display_name == "Inbox").unwrap();
    assert_eq!(inbox.well_known, Some("inbox"));
    let old = folders.iter().find(|f| f.display_name == "Old").unwrap();
    assert_eq!(old.parent_folder_id.as_deref(), Some("F-projects"));

    let mut mirror = BTreeMap::new();
    for f in &folders {
        let (seen, _) = round(&graph, &f.id, None);
        for (id, here) in seen {
            assert!(here);
            assert!(mirror.insert(id, f.id.clone()).is_none(), "one folder each");
        }
    }
    assert_eq!(mirror, fake.messages());

    let id = FakeGraph::id_of(4);
    let mut raw = Vec::new();
    let n = graph.raw(TOKEN, &id, &mut raw).unwrap();
    assert_eq!(raw, mailbox.raw(4));
    assert_eq!(n, raw.len() as u64);
}

#[test]
fn a_move_keeps_the_id_and_a_delete_is_removed() {
    let (fake, graph, _) = setup(120);
    let inbox = FakeGraph::folder_id("Inbox");
    let archive = FakeGraph::folder_id("Archive");
    let (first, in_link) = round(&graph, inbox, None);
    let (_, ar_link) = round(&graph, archive, None);
    let moved = first.keys().next().unwrap().clone();
    let deleted = first.keys().nth(1).unwrap().clone();
    let read = first.keys().nth(2).unwrap().clone();
    assert!(fake.move_message(&moved, archive));
    assert!(fake.delete_message(&deleted));
    assert!(fake.set_read(&read, false));
    let added = fake.add_message(b"Subject: new\r\n\r\nhello\r\n", inbox);

    let (inbox_changes, _) = round(&graph, inbox, Some(in_link));
    let want: BTreeMap<String, bool> = [
        (moved.clone(), false),
        (deleted, false),
        (read, true),
        (added, true),
    ]
    .into_iter()
    .collect();
    assert_eq!(inbox_changes, want);
    let (archive_changes, _) = round(&graph, archive, Some(ar_link));
    assert_eq!(archive_changes, [(moved, true)].into_iter().collect());
}

#[test]
fn an_expired_link_asks_for_a_new_round() {
    let (fake, graph, _) = setup(40);
    let inbox = FakeGraph::folder_id("Inbox");
    let (_, link) = round(&graph, inbox, None);
    fake.add_message(b"Subject: x\r\n\r\ny\r\n", inbox);
    fake.expire_delta_before(fake.seq() + 1);
    assert!(matches!(
        graph.delta(TOKEN, inbox, Some(&link)),
        Err(Error::DeltaReset)
    ));
    // A new round with no link works.
    round(&graph, inbox, None);
}

#[test]
fn a_link_to_another_host_sends_no_request() {
    let (fake, graph, _) = setup(60);
    fake.set_page_limit(5);
    fake.set_link_host("http://127.0.0.1:1");
    let inbox = FakeGraph::folder_id("Inbox");
    let page = graph.delta(TOKEN, inbox, None);
    assert!(matches!(page, Err(Error::Decode(_))), "{page:?}");
    assert_eq!(fake.requests().len(), 1);
}

#[test]
fn at_most_four_requests_run_at_the_same_time() {
    let (fake, graph, _) = setup(20);
    fake.set_delay(Duration::from_millis(40));
    std::thread::scope(|s| {
        for i in 0..10 {
            let graph = &graph;
            s.spawn(move || {
                let mut sink = Vec::new();
                graph.raw(TOKEN, &FakeGraph::id_of(i), &mut sink).unwrap();
            });
        }
    });
    assert_eq!(fake.most_running(), iris_graph::MAX_RUNNING);
}

#[test]
fn a_read_waits_after_429_and_503() {
    let (fake, graph, _) = setup(5);
    fake.fail_next(&[(429, Some(0)), (503, None)]);
    let me = graph.me(TOKEN).unwrap();
    assert_eq!(me.address(), iris_synth::OWNER);
    assert_eq!(fake.requests().len(), 3);
}

#[test]
fn errors_keep_the_graph_code() {
    let (fake, graph, _) = setup(5);
    fake.fail_next(&[(400, None)]);
    match graph.me(TOKEN) {
        Err(Error::Api { status, code, .. }) => {
            assert_eq!((status, code.as_str()), (400, "Injected"))
        }
        other => panic!("{other:?}"),
    }
    fake.fail_next(&[(401, None)]);
    assert!(matches!(graph.me(TOKEN), Err(Error::Unauthorized)));
    assert!(matches!(
        graph.raw(TOKEN, "AAMkffffffff", &mut Vec::new()),
        Err(Error::NotFound)
    ));
}
