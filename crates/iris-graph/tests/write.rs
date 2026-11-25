//! The write calls against the fake Graph server (spec section 5,
//! acceptance criteria 6 and 7).

use std::time::Duration;

use iris_fakegraph::FakeGraph;
use iris_graph::{Config, Error, Graph};
use iris_synth::Mailbox;

const TOKEN: &str = "test-token";

fn setup() -> (FakeGraph, Graph) {
    let fake = FakeGraph::start(Mailbox::new(7, 30)).unwrap();
    let graph = Graph::new(Config {
        base_url: fake.base_url(),
        backoff_start: Duration::from_millis(5),
        backoff_max: Duration::from_millis(50),
        ..Config::default()
    });
    (fake, graph)
}

fn inbox_message(fake: &FakeGraph) -> String {
    let inbox = FakeGraph::folder_id("Inbox");
    fake.messages()
        .into_iter()
        .find(|(_, f)| f == inbox)
        .unwrap()
        .0
}

const MIME: &[u8] = b"From: owner@example.com\r\nTo: a@example.com\r\nSubject: Hi\r\n\
Message-ID: <iris-1'x@example.com>\r\n\r\nHello.\r\n";

type Write<'a> = Box<dyn Fn() -> Result<(), Error> + 'a>;

/// Each write, by name. The category write comes first, because the
/// other writes change the etag.
fn writes<'a>(fake: &'a FakeGraph, graph: &'a Graph) -> Vec<(&'static str, Write<'a>)> {
    let id = inbox_message(fake);
    let etag = fake.etag_of(&id).unwrap();
    let archive = FakeGraph::folder_id("Archive");
    let (i1, i2, i3, i4) = (id.clone(), id.clone(), id.clone(), id);
    vec![
        (
            "set_categories",
            Box::new(move || {
                graph
                    .set_categories(TOKEN, &i3, &["Red".to_string()], &etag)
                    .map(drop)
            }),
        ),
        (
            "set_read",
            Box::new(move || graph.set_read(TOKEN, &i1, true, None).map(drop)),
        ),
        (
            "set_flag",
            Box::new(move || graph.set_flag(TOKEN, &i2, true, None).map(drop)),
        ),
        (
            "move_to",
            Box::new(move || {
                // The etag of now: the writes before change it.
                let now = fake.etag_of(&i4).unwrap_or_default();
                graph.move_to(TOKEN, &i4, archive, &now).map(drop)
            }),
        ),
        (
            "create_draft",
            Box::new(move || graph.create_draft(TOKEN, MIME).map(drop)),
        ),
        ("send_mime", Box::new(move || graph.send_mime(TOKEN, MIME))),
    ]
}

#[test]
fn each_write_makes_one_request_also_after_429_and_503() {
    for status in [429, 503, 500] {
        let (fake, graph) = setup();
        for (name, write) in writes(&fake, &graph) {
            let before = fake.requests().len();
            fake.fail_next(&[(status, Some(0))]);
            let got = write();
            let err = got.expect_err(name);
            assert!(err.write_result_unknown(), "{name} {status}: {err}");
            assert_eq!(fake.requests().len(), before + 1, "{name} {status}");
        }
        assert!(fake.sent_mail().is_empty(), "no send after a refusal");
    }
}

#[test]
fn a_401_of_a_write_is_definite_and_each_write_works_after() {
    let (fake, graph) = setup();
    for (name, write) in writes(&fake, &graph) {
        fake.fail_next(&[(401, None)]);
        let err = write().expect_err(name);
        assert!(matches!(err, Error::Unauthorized), "{name}: {err}");
        assert!(!err.write_result_unknown(), "{name}");
    }
    for (name, write) in writes(&fake, &graph) {
        write().unwrap_or_else(|e| panic!("{name}: {e}"));
    }
}

#[test]
fn the_state_writes_change_the_fake_and_answer_the_new_state() {
    let (fake, graph) = setup();
    let id = inbox_message(&fake);
    assert!(!graph.set_read(TOKEN, &id, false, None).unwrap().is_read);
    assert!(graph.set_read(TOKEN, &id, true, None).unwrap().is_read);
    assert!(graph.set_flag(TOKEN, &id, true, None).unwrap().flagged());
    assert!(!graph.set_flag(TOKEN, &id, false, None).unwrap().flagged());
    let archive = FakeGraph::folder_id("Archive");
    let old = fake.etag_of(&id).unwrap();
    assert!(graph.set_read(TOKEN, &id, true, None).is_ok());
    // The move has a condition: an old etag moves nothing.
    match graph.move_to(TOKEN, &id, archive, &old) {
        Err(Error::Api { status: 412, .. }) => {}
        other => panic!("a move with an old etag: {other:?}"),
    }
    assert_ne!(fake.messages()[&id], archive);
    let now = fake.etag_of(&id).unwrap();
    let moved = graph.move_to(TOKEN, &id, archive, &now).unwrap();
    assert_eq!(
        (moved.id.as_str(), moved.parent_folder_id.as_str()),
        (id.as_str(), archive)
    );
    assert_eq!(fake.messages()[&id], archive);
    let m = graph.message(TOKEN, &id).unwrap();
    assert!(m.is_read && !m.flagged());
}

#[test]
fn an_old_etag_gets_412_and_changes_nothing() {
    let (fake, graph) = setup();
    let id = inbox_message(&fake);
    let old = graph.message(TOKEN, &id).unwrap().etag.unwrap();
    let before = graph.message(TOKEN, &id).unwrap().categories;
    // Another client changes the message after the read.
    assert!(fake.set_read(&id, true));
    let err = graph
        .set_categories(TOKEN, &id, &["Red".to_string()], &old)
        .unwrap_err();
    assert!(matches!(err, Error::Api { status: 412, .. }), "{err}");
    assert!(!err.write_result_unknown());
    assert_eq!(graph.message(TOKEN, &id).unwrap().categories, before);
    // A new read and a new write with its etag work.
    let new = graph.draft_etag(TOKEN, &id).unwrap();
    assert_ne!(new, old);
    let list = vec!["Red".to_string(), "Blue category".to_string()];
    let m = graph.set_categories(TOKEN, &id, &list, &new).unwrap();
    assert_eq!(m.categories, list);
}

#[test]
fn a_draft_keeps_the_bytes_and_the_send_sends_the_given_bytes() {
    let (fake, graph) = setup();
    let draft = graph.create_draft(TOKEN, MIME).unwrap();
    assert!(draft.is_draft);
    assert_eq!(draft.parent_folder_id, FakeGraph::folder_id("Drafts"));
    assert_eq!(
        graph.draft_etag(TOKEN, &draft.id).unwrap(),
        draft.etag.clone().unwrap()
    );
    let mut raw = Vec::new();
    graph.raw(TOKEN, &draft.id, &mut raw).unwrap();
    assert_eq!(raw, MIME);

    graph.send_mime(TOKEN, MIME).unwrap();
    assert_eq!(fake.sent_mail(), vec![MIME.to_vec()]);
    // The search finds the draft and the sent copy, over two pages. The
    // quote in the Message-ID is escaped in the filter.
    let copies = graph
        .sent_by_message_id(TOKEN, "<iris-1'x@example.com>")
        .unwrap();
    assert_eq!(copies.len(), 2, "{copies:?}");
    let sent: Vec<_> = copies
        .iter()
        .filter(|c| c.id != draft.id && !c.is_draft && c.sent_date_time.is_some())
        .collect();
    assert_eq!(sent.len(), 1, "{copies:?}");
    assert_eq!(sent[0].parent_folder_id, FakeGraph::folder_id("Sent Items"));
    assert!(graph
        .sent_by_message_id(TOKEN, "<none@example.com>")
        .unwrap()
        .is_empty());
}

#[test]
fn a_bad_answer_to_an_accepted_write_is_unknown() {
    let (fake, graph) = setup();
    let id = inbox_message(&fake);
    fake.answer_next(&["{}"]);
    let err = graph.set_read(TOKEN, &id, true, None).unwrap_err();
    assert!(err.write_result_unknown(), "{err}");
    // An answer with another state is unknown too.
    let other = format!(
        r#"{{"id":"{id}","conversationId":"c","parentFolderId":"F-inbox","categories":[],
        "isRead":false,"flag":{{"flagStatus":"notFlagged"}},"isDraft":false}}"#
    );
    fake.answer_next(&[&other]);
    let err = graph.set_read(TOKEN, &id, true, None).unwrap_err();
    assert!(err.write_result_unknown(), "{err}");
}

#[test]
fn definite_and_unknown_errors() {
    let api = |status| Error::Api {
        status,
        code: String::new(),
        message: String::new(),
    };
    for s in [302, 429, 500, 503] {
        assert!(api(s).write_result_unknown(), "{s}");
    }
    for s in [400, 403, 409, 412] {
        assert!(!api(s).write_result_unknown(), "{s}");
    }
    assert!(Error::Transport("cut".into()).write_result_unknown());
    assert!(!Error::NotFound.write_result_unknown());
    assert!(!Error::Unauthorized.write_result_unknown());
    assert!(!Error::Decode("x".into()).write_result_unknown());
}

/// An accepted answer, with `changes` on top of a plain inbox message.
fn answer(id: &str, changes: serde_json::Value) -> String {
    let mut m = serde_json::json!({
        "id": id, "conversationId": "c", "parentFolderId": "F-inbox",
        "categories": [], "isRead": true, "flag": {"flagStatus": "notFlagged"},
        "isDraft": false, "@odata.etag": "W/\"9\"",
    });
    for (k, v) in changes.as_object().unwrap() {
        if v.is_null() {
            m.as_object_mut().unwrap().remove(k);
        } else {
            m[k] = v.clone();
        }
    }
    m.to_string()
}

#[test]
fn each_write_answer_must_show_exactly_the_asked_state() {
    use serde_json::json;
    let (fake, graph) = setup();
    let id = inbox_message(&fake);
    let list = vec!["Red".to_string(), "Blue".to_string()];
    // The same categories in another order: success.
    fake.answer_next(&[&answer(&id, json!({"categories": ["Blue", "Red"]}))]);
    graph.set_categories(TOKEN, &id, &list, "W/\"1\"").unwrap();
    // One category less: unknown.
    fake.answer_next(&[&answer(&id, json!({"categories": ["Red"]}))]);
    let err = graph
        .set_categories(TOKEN, &id, &list, "W/\"1\"")
        .unwrap_err();
    assert!(err.write_result_unknown(), "{err}");
    // An unflag answered with `complete`: unknown.
    fake.answer_next(&[&answer(&id, json!({"flag": {"flagStatus": "complete"}}))]);
    let err = graph.set_flag(TOKEN, &id, false, None).unwrap_err();
    assert!(err.write_result_unknown(), "{err}");
    fake.answer_next(&[&answer(&id, json!({}))]);
    graph.set_flag(TOKEN, &id, false, None).unwrap();
    // A draft with no etag, or with an empty ID: unknown.
    for changes in [
        json!({"isDraft": true, "@odata.etag": null}),
        json!({"isDraft": true, "@odata.etag": ""}),
        json!({"isDraft": true, "id": ""}),
    ] {
        fake.answer_next_status(201, &answer(&id, changes.clone()));
        let err = graph.create_draft(TOKEN, MIME).unwrap_err();
        assert!(err.write_result_unknown(), "{changes} {err}");
    }
    fake.answer_next_status(201, &answer(&id, json!({"isDraft": true})));
    graph.create_draft(TOKEN, MIME).unwrap();
}

#[test]
fn a_2xx_other_than_the_success_status_is_unknown() {
    use serde_json::json;
    let (fake, graph) = setup();
    let id = inbox_message(&fake);
    let archive = FakeGraph::folder_id("Archive");
    let draft = answer(&id, json!({"isDraft": true}));
    let moved = answer(&id, json!({"parentFolderId": archive}));
    // sendMail succeeds with 202 only.
    for status in [200, 201, 204] {
        fake.answer_next_status(status, "{}");
        let err = graph.send_mime(TOKEN, MIME).unwrap_err();
        assert!(err.write_result_unknown(), "{status} {err}");
    }
    // A draft and a move succeed with 201 only.
    fake.answer_next_status(200, &draft);
    assert!(graph
        .create_draft(TOKEN, MIME)
        .unwrap_err()
        .write_result_unknown());
    fake.answer_next_status(200, &moved);
    assert!(graph
        .move_to(TOKEN, &id, archive, "W/\"1\"")
        .unwrap_err()
        .write_result_unknown());
    // A PATCH succeeds with 200 only.
    fake.answer_next_status(201, &answer(&id, json!({})));
    assert!(graph
        .set_read(TOKEN, &id, true, None)
        .unwrap_err()
        .write_result_unknown());
    assert!(fake.sent_mail().is_empty());
    // The right statuses pass.
    fake.answer_next_status(202, "");
    graph.send_mime(TOKEN, MIME).unwrap();
    fake.answer_next_status(201, &moved);
    graph.move_to(TOKEN, &id, archive, "W/\"1\"").unwrap();
}

/// Issue #883: an injected answer must not leave the request body unread.
/// Else the fake server closes the connection after the answer, the next
/// write on the pooled connection fails at transport, and the queue of
/// answers shifts by one request.
#[test]
fn an_injected_answer_keeps_the_connection_for_the_next_write() {
    use serde_json::json;
    let (fake, graph) = setup();
    let id = inbox_message(&fake);
    // A body that the server cannot have read when it answers.
    let big = [MIME, &vec![b'x'; 4 << 20][..]].concat();
    for _ in 0..5 {
        fake.answer_next_status(201, &answer(&id, json!({"isDraft": true, "id": ""})));
        let err = graph.create_draft(TOKEN, &big).unwrap_err();
        assert!(matches!(err, iris_graph::Error::Differs(_)), "{err}");
        fake.answer_next_status(201, &answer(&id, json!({"isDraft": true})));
        graph.create_draft(TOKEN, MIME).unwrap();
    }
}
