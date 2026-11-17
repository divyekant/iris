//! The apply of the state verbs `close`, `decline`, `defer` and `correct`
//! (specification 2, section 4): the item, the correction rows and the
//! ledger entry, in one transaction.

use iris_apply::{apply_state, apply_states, Outcome};
use iris_changeset::{ActionId, Author, AuthorKind, Verb};
use iris_store::{NewMessage, Store};
use serde_json::{json, Map, Value};

const APPLY_POLICY: &str = "version = 0\nmode = \"apply\"\n";

struct Box {
    store: Store,
    dir: std::path::PathBuf,
}

impl Drop for Box {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

/// A store with one message and the commitment `c-1` (the example of the
/// open model) with `status` (`open`, `done`, ...). `policy` is the text
/// of the active policy, or the mode `observe` of the first start.
fn setup(name: &str, status: &str, policy: Option<&str>) -> Box {
    let dir = std::env::temp_dir().join(format!("iris-state-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    let a = store.add_account("gmail", "owner@example.com").unwrap();
    let m = NewMessage {
        provider_id: "g1".into(),
        thread_provider_id: "gt1".into(),
        ..Default::default()
    };
    store.put_messages(a, &[m]).unwrap();
    store
        .write(|tx| {
            let mut doc = iris_spec::read_json(
                &iris_spec::schema_dir()
                    .join("examples")
                    .join("commitment-you-owe.json"),
            );
            doc["id"] = json!("c-1");
            doc["status"] = json!(status);
            tx.execute(
                "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
                 VALUES ('commitment', ?1, ?2, ?3, 0, 0)",
                rusqlite::params![a, status, doc.to_string()],
            )?;
            tx.execute(
                "INSERT INTO item_source(item_id, message_id, role) VALUES (1, 1, 'origin')",
                [],
            )?;
            if let Some(text) = policy {
                tx.execute(
                    "INSERT INTO policy_version(text, sha256, confirmed_at) VALUES (?1, 'h', 0)",
                    [text],
                )?;
                tx.execute("INSERT INTO policy_rule_id(id) VALUES (1)", [])?;
            }
            Ok(())
        })
        .unwrap();
    Box { store, dir }
}

impl Box {
    /// A submitted action of `verb` on `c-1` by `kind`. Gives the action
    /// ID and the changeset.
    fn submitted(&self, kind: AuthorKind, verb: Verb, args: Value) -> (i64, ActionId) {
        let author = Author {
            kind,
            name: None,
            session: None,
            job: None,
        };
        let args: Map<String, Value> = args.as_object().unwrap().clone();
        let s = &self.store;
        let cs = iris_changeset::create(s, &author, now()).unwrap();
        iris_changeset::add_action(s, cs, &author, verb, &["c-1"], &args, None).unwrap();
        iris_changeset::submit(s, cs, &author, "The work is done.", now()).unwrap();
        let id = self.one("SELECT max(id) FROM changeset_action");
        (
            id.as_i64().unwrap(),
            ActionId {
                changeset: cs,
                revision: 1,
                n: 1,
            },
        )
    }

    /// As `submitted`, approved by the person with the hash of the review.
    fn approved(&self, verb: Verb, args: Value) -> i64 {
        let (id, action) = self.submitted(AuthorKind::Person, verb, args);
        let hash = self.one(&format!(
            "SELECT hash FROM changeset_action WHERE id = {id}"
        ));
        iris_changeset::approve(&self.store, action, hash.as_str().unwrap(), now()).unwrap();
        id
    }

    /// The first column of the first row, as JSON.
    fn one(&self, sql: &str) -> Value {
        self.store
            .read(|c| {
                Ok(c.query_row(sql, [], |r| {
                    Ok(match r.get_ref(0)? {
                        rusqlite::types::ValueRef::Null => Value::Null,
                        rusqlite::types::ValueRef::Integer(i) => json!(i),
                        rusqlite::types::ValueRef::Text(t) => {
                            json!(String::from_utf8_lossy(t))
                        }
                        other => panic!("{other:?}"),
                    })
                })?)
            })
            .unwrap()
    }

    fn doc(&self) -> Value {
        let d = self.one("SELECT doc FROM item WHERE id = 1");
        serde_json::from_str(d.as_str().unwrap()).unwrap()
    }

    fn state(&self, action: i64) -> String {
        let s = self.one(&format!(
            "SELECT state FROM changeset_action WHERE id = {action}"
        ));
        s.as_str().unwrap().into()
    }

    /// The ledger entry of the action: state, undo, pre and post.
    fn entry(&self, action: i64) -> (String, String, Value, Value) {
        let col = |c: &str| {
            self.one(&format!(
                "SELECT {c} FROM ledger WHERE action_id = {action}"
            ))
        };
        let json = |v: Value| {
            v.as_str()
                .map_or(Value::Null, |t| serde_json::from_str(t).unwrap())
        };
        (
            col("state").as_str().unwrap().into(),
            col("undo").as_str().unwrap().into(),
            json(col("pre")),
            json(col("post")),
        )
    }
}

/// Each verb sets its status, the document, a correction row for each
/// field with the changeset as `by`, and an applied ledger entry with the
/// old and new values. The approval of the person applies in the mode
/// `observe` too: a state verb changes no mailbox.
#[test]
fn each_verb_changes_the_commitment() {
    for (verb, args, status) in [
        (Verb::Close, json!({ "reason": "Sent it." }), "done"),
        (Verb::Decline, json!({ "reason": "Not mine." }), "declined"),
        (Verb::Defer, json!({ "date": "2026-12-01" }), "deferred"),
    ] {
        let b = setup(verb.as_str(), "open", None);
        let a = b.approved(verb, args);
        assert_eq!(
            apply_state(&b.store, a).unwrap(),
            Outcome::Applied(Default::default())
        );
        assert_eq!(b.state(a), "applied");
        assert_eq!(b.one("SELECT status FROM item WHERE id = 1"), json!(status));
        let doc = b.doc();
        assert_eq!(doc["status"], json!(status));
        assert_ne!(doc["updated"], json!("2026-11-16T09:02:11Z"));
        let (state, undo, pre, post) = b.entry(a);
        assert_eq!((state.as_str(), undo.as_str()), ("applied", "inverse"));
        if verb == Verb::Defer {
            assert_eq!(doc["deferred_until"], json!("2026-12-01"));
            assert_eq!(
                pre,
                json!({ "c-1": { "status": "open", "deferred_until": null } })
            );
            assert_eq!(
                post,
                json!({ "c-1": { "status": "deferred", "deferred_until": "2026-12-01" } })
            );
        } else {
            assert!(doc.get("deferred_until").is_none());
            assert_eq!(
                pre,
                json!({ "c-1": { "status": "open", "deferred_until": null } })
            );
            assert_eq!(
                post,
                json!({ "c-1": { "status": status, "deferred_until": null } })
            );
        }
        let fields = if verb == Verb::Defer { 2 } else { 1 };
        assert_eq!(
            b.one("SELECT count(*) FROM correction WHERE item_id = 1 AND by = 'cs-1'"),
            json!(fields)
        );
        assert_eq!(
            b.one("SELECT value FROM correction WHERE field = 'status'"),
            json!(format!("\"{status}\""))
        );
        // The document stays valid against the open model.
        iris_understand::validate(&doc).unwrap();
    }
}

/// A deferred commitment can close. A done one cannot: the entry fails
/// with the reason, no undo, and the item does not change.
#[test]
fn only_an_open_or_deferred_commitment_changes() {
    let b = setup("deferred", "deferred", None);
    b.store
        .write(|tx| {
            Ok(tx.execute(
                "UPDATE item SET doc = json_set(doc, '$.deferred_until', '2026-12-01')",
                [],
            )?)
        })
        .unwrap();
    let a = b.approved(Verb::Close, json!({ "reason": "Done." }));
    assert!(matches!(apply_state(&b.store, a), Ok(Outcome::Applied(_))));
    assert!(b.doc().get("deferred_until").is_none());
    // The undo needs the old date.
    let (_, _, pre, _) = b.entry(a);
    assert_eq!(
        pre,
        json!({ "c-1": { "status": "deferred", "deferred_until": "2026-12-01" } })
    );

    let b = setup("done", "done", None);
    let before = b.doc();
    let a = b.approved(Verb::Decline, json!({ "reason": "No." }));
    let Ok(Outcome::Failed { error, .. }) = apply_state(&b.store, a) else {
        panic!("not failed");
    };
    assert!(error.contains("c-1 is done"), "{error}");
    assert_eq!(b.state(a), "failed");
    let (state, undo, pre, post) = b.entry(a);
    assert_eq!((state.as_str(), undo.as_str()), ("failed", "none"));
    assert_eq!((pre, post), (Value::Null, Value::Null));
    assert_eq!(b.doc(), before);
    assert_eq!(b.one("SELECT count(*) FROM correction"), json!(0));
}

/// `correct` writes the field through the correction rules, with the
/// changeset as `by`.
#[test]
fn correct_changes_one_field() {
    let b = setup("correct", "open", None);
    let a = b.approved(
        Verb::Correct,
        json!({ "field": "due.date", "value": "2026-11-20" }),
    );
    assert!(matches!(apply_state(&b.store, a), Ok(Outcome::Applied(_))));
    assert_eq!(b.doc()["due"]["date"], json!("2026-11-20"));
    let (_, _, pre, post) = b.entry(a);
    assert_eq!(
        pre,
        json!({ "c-1": { "due.date": "2026-11-17", "status": "open" } })
    );
    assert_eq!(
        post,
        json!({ "c-1": { "due.date": "2026-11-20", "status": "open" } })
    );
    assert_eq!(
        b.one("SELECT by FROM correction WHERE field = 'due.date'"),
        json!("cs-1")
    );

    // The due date is the only change: no `status` correction.
    assert_eq!(
        b.one("SELECT count(*) FROM correction WHERE field = 'status'"),
        json!(0)
    );

    // A correction can change the status too. The entry holds that change,
    // so the undo can set it back, and a `status` correction records it
    // (spec 1 section 7.5): a record becomes `corrected`, and a signal
    // follows its new `expires`.
    for (example, form, status, id, field, old, value, new) in [
        (
            "record-receipt",
            "record",
            "kept",
            "r-2",
            "fields.order",
            "EB-20931",
            "EB-1",
            "corrected",
        ),
        (
            "signal-code",
            "signal",
            "expired",
            "s-3",
            "expires",
            "2026-11-16T09:15:00Z",
            "2099-01-01T00:00:00Z",
            "active",
        ),
    ] {
        let a = b.correct_example(example, form, status, id, field, value);
        assert!(matches!(apply_state(&b.store, a), Ok(Outcome::Applied(_))));
        let (_, _, pre, post) = b.entry(a);
        assert_eq!(pre, json!({ id: { field: old, "status": status } }));
        assert_eq!(post, json!({ id: { field: value, "status": new } }));
        let cs = b.one(&format!(
            "SELECT 'cs-' || changeset_id FROM changeset_action WHERE id = {a}"
        ));
        let rows = b.one(&format!(
            "SELECT json_group_array(json_array(field, json(value), by)) FROM (
               SELECT c.* FROM correction c JOIN item i ON i.id = c.item_id
               WHERE json_extract(i.doc, '$.id') = '{id}' ORDER BY c.id)"
        ));
        let rows: Value = serde_json::from_str(rows.as_str().unwrap()).unwrap();
        assert_eq!(rows, json!([[field, value, cs], ["status", new, cs]]));
    }
}

impl Box {
    /// Insert the schema example `example` as the item `id` with the status
    /// `status`, and give a person-approved `correct` action of `field`.
    fn correct_example(
        &self,
        example: &str,
        form: &str,
        status: &str,
        id: &str,
        field: &str,
        value: &str,
    ) -> i64 {
        let mut doc = iris_spec::read_json(
            &iris_spec::schema_dir()
                .join("examples")
                .join(format!("{example}.json")),
        );
        doc["id"] = json!(id);
        doc["status"] = json!(status);
        self.store
            .write(|tx| {
                Ok(tx.execute(
                    "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
                     VALUES (?1, 1, ?2, ?3, 0, 0)",
                    rusqlite::params![form, status, doc.to_string()],
                )?)
            })
            .unwrap();
        let author = Author {
            kind: AuthorKind::Person,
            name: None,
            session: None,
            job: None,
        };
        let args = json!({ "field": field, "value": value });
        let s = &self.store;
        let cs = iris_changeset::create(s, &author, now()).unwrap();
        iris_changeset::add_action(
            s,
            cs,
            &author,
            Verb::Correct,
            &[id],
            args.as_object().unwrap(),
            None,
        )
        .unwrap();
        iris_changeset::submit(s, cs, &author, "Fix it.", now()).unwrap();
        let a = self
            .one("SELECT max(id) FROM changeset_action")
            .as_i64()
            .unwrap();
        let hash = self.one(&format!("SELECT hash FROM changeset_action WHERE id = {a}"));
        let action = ActionId {
            changeset: cs,
            revision: 1,
            n: 1,
        };
        iris_changeset::approve(s, action, hash.as_str().unwrap(), now()).unwrap();
        a
    }
}

/// In the mode `observe`, an action that a person did not approve waits.
/// In the mode `apply`, a rule approval goes to review (issue #168). The
/// tick applies each approved state action.
#[test]
fn only_the_person_approval_applies() {
    let rule = "decided_by = 'rule', rule = 1, policy = 1, decided_at = 2, state = 'approved'";
    for (policy, want) in [(None, "approved"), (Some(APPLY_POLICY), "in_review")] {
        let b = setup("rule", "open", policy);
        let (a, _) = b.submitted(AuthorKind::Agent, Verb::Close, json!({ "reason": "Done." }));
        if policy.is_none() {
            // A rule of an earlier policy approved it: the version row.
            b.store
                .write(|tx| {
                    tx.execute_batch(
                        "INSERT INTO policy_version(text, sha256, confirmed_at)
                           VALUES ('version = 0\nmode = \"observe\"\n', 'h', 0);
                         INSERT INTO policy_rule_id(id) VALUES (1);",
                    )?;
                    Ok(())
                })
                .unwrap();
        }
        b.store
            .write(|tx| {
                Ok(tx.execute(
                    &format!("UPDATE changeset_action SET {rule} WHERE id = ?1"),
                    [a],
                )?)
            })
            .unwrap();
        assert_eq!(apply_states(&b.store).unwrap(), 0);
        assert_eq!(b.state(a), want);
        assert_eq!(b.one("SELECT status FROM item WHERE id = 1"), json!("open"));
    }
    let b = setup("tick", "open", Some(APPLY_POLICY));
    let a = b.approved(Verb::Close, json!({ "reason": "Done." }));
    assert_eq!(apply_states(&b.store).unwrap(), 1);
    assert_eq!(b.state(a), "applied");
    assert_eq!(apply_states(&b.store).unwrap(), 0);
}

/// The tick reads the actions in pages of 100: each of 250 actions gets
/// its entry. The first close applies, and each later one fails, because
/// the commitment is done.
#[test]
fn the_tick_reads_the_actions_in_pages() {
    let b = setup("pages", "open", Some(APPLY_POLICY));
    for _ in 0..250 {
        b.approved(Verb::Close, json!({ "reason": "Done." }));
    }
    assert_eq!(apply_states(&b.store).unwrap(), 1);
    assert_eq!(b.one("SELECT count(*) FROM ledger"), json!(250));
    assert_eq!(
        b.one("SELECT count(*) FROM changeset_action WHERE state = 'failed'"),
        json!(249)
    );
}

/// A close of a job session applies inside the grant. After a revoke, the
/// apply denies it in the transaction of the item writes, and the item does
/// not change (docs/specs/2026-11-17-job-verbs.md).
#[test]
fn a_session_close_applies_only_inside_the_grant() {
    for revoked in [false, true] {
        let b = setup(&format!("grant-{revoked}"), "open", None);
        b.store
            .write(|tx| {
                tx.execute_batch(
                    "UPDATE message SET understood_at = 1;
                     INSERT OR IGNORE INTO header(message_id, from_addr)
                       VALUES (1, 'dana@example.com');
                     UPDATE header SET from_addr = 'dana@example.com';",
                )?;
                Ok(())
            })
            .unwrap();
        let job = iris_job::create(&b.store, "Close it.", None, now()).unwrap();
        iris_job::add_scope(
            &b.store,
            job,
            iris_job::Ref::Thread(1),
            iris_job::By::Person,
            now(),
        )
        .unwrap();
        let me = Author {
            kind: AuthorKind::Agent,
            name: Some("claude-code".into()),
            session: Some("se-1".into()),
            job: Some(iris_job::format_job_id(job)),
        };
        let s = &b.store;
        let mut args = Map::new();
        args.insert("reason".into(), "Done.".into());
        let cs = iris_changeset::create(s, &me, now()).unwrap();
        iris_changeset::add_action(s, cs, &me, Verb::Close, &["c-1"], &args, None).unwrap();
        iris_changeset::submit(s, cs, &me, "Signed.", now()).unwrap();
        let id = b
            .one("SELECT max(id) FROM changeset_action")
            .as_i64()
            .unwrap();
        let hash = b.one(&format!(
            "SELECT hash FROM changeset_action WHERE id = {id}"
        ));
        let action = ActionId {
            changeset: cs,
            revision: 1,
            n: 1,
        };
        iris_changeset::approve(s, action, hash.as_str().unwrap(), now()).unwrap();
        if revoked {
            iris_job::revoke_scope(
                s,
                job,
                iris_job::Ref::Thread(1),
                iris_job::By::Person,
                now(),
            )
            .unwrap();
        }
        match (revoked, apply_state(s, id).unwrap()) {
            (false, Outcome::Applied(_)) => assert_eq!(b.doc()["status"], json!("done")),
            (true, Outcome::Denied(why)) => {
                assert!(why.contains("outside the grant"), "{why}");
                assert_eq!(b.doc()["status"], json!("open"));
                assert_eq!(b.state(id), "denied");
            }
            other => panic!("{other:?}"),
        }
    }
}

impl Box {
    /// The doc of c-1 with no `updated` (the undo sets a new time) and no
    /// sender row ID (a `correct` writes the sender rows of the store).
    fn plain(&self) -> Value {
        let mut d = self.doc();
        d.as_object_mut().unwrap().remove("updated");
        d["counterparty"].as_object_mut().unwrap().remove("sender");
        d
    }

    fn ledger_of(&self, action: i64) -> i64 {
        self.one(&format!("SELECT id FROM ledger WHERE action_id = {action}"))
            .as_i64()
            .unwrap()
    }

    /// Make the undo of the entry of `action`, and apply its one action.
    fn undo(&self, action: i64) -> (i64, Outcome) {
        let ids = iris_apply::undo(&self.store, self.ledger_of(action), now()).unwrap();
        assert_eq!(ids.len(), 1, "a state undo has one action");
        assert_eq!(
            self.one(&format!(
                "SELECT verb FROM changeset_action WHERE id = {}",
                ids[0]
            )),
            json!("restore")
        );
        (ids[0], apply_state(&self.store, ids[0]).unwrap())
    }
}

/// The undo of each state verb (issue #226) gives the item its document,
/// its status and its corrections of before, and makes the entry `undone`.
#[test]
fn the_undo_of_each_state_verb_sets_the_old_values() {
    for (i, (verb, args)) in [
        (Verb::Close, json!({ "reason": "Done." })),
        (Verb::Decline, json!({ "reason": "No." })),
        (Verb::Defer, json!({ "date": "2099-01-05" })),
        (
            Verb::Correct,
            json!({ "field": "due.date", "value": "2026-11-20" }),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let b = setup(&format!("undo{i}"), "open", None);
        let before = b.plain();
        let entry_can = |b: &Box, a: i64| {
            b.one(&format!(
                "SELECT count(*) FROM ledger_can_undo WHERE ledger_id = {}",
                b.ledger_of(a)
            ))
        };
        let a = b.approved(verb, args.clone());
        assert!(matches!(apply_state(&b.store, a), Ok(Outcome::Applied(_))));
        assert_ne!(b.plain(), before, "{verb:?} changed the item");
        assert_eq!(
            entry_can(&b, a),
            json!(1),
            "{verb:?}: the entry has an undo"
        );
        let (undo, outcome) = b.undo(a);
        let Outcome::Applied(counts) = outcome else {
            panic!("{outcome:?}")
        };
        assert_eq!((counts.changed, counts.unchanged), (1, 0), "{verb:?}");
        assert_eq!(b.plain(), before, "{verb:?}: the document of before");
        assert_eq!(b.one("SELECT status FROM item WHERE id = 1"), json!("open"));
        assert_eq!(
            b.one("SELECT count(*) FROM correction WHERE by = 'cs-1'"),
            json!(0),
            "{verb:?}: no correction of the entry"
        );
        let (state, _, _, _) = b.entry(a);
        assert_eq!(state, "undone", "{verb:?}");
        assert_eq!(b.state(a), "undone");
        assert_eq!(b.state(undo), "applied");
        assert_eq!(
            b.one(&format!(
                "SELECT undone_by FROM ledger WHERE action_id = {a}"
            )),
            json!(b.ledger_of(undo))
        );
        assert_eq!(entry_can(&b, a), json!(0), "{verb:?}: no second undo");
        // The undo changeset is a valid document of the open model.
        let cs = b.one(&format!("SELECT changeset_id FROM changeset_action WHERE id = {undo}"));
        let mut out = Vec::new();
        iris_changeset::write_document(&b.store, cs.as_i64().unwrap(), &mut out).unwrap();
        let doc: Value = serde_json::from_slice(&out).unwrap();
        let errors: Vec<String> = iris_spec::validator("changeset")
            .iter_errors(&doc)
            .map(|e| format!("{e} at {}", e.instance_path()))
            .collect();
        assert!(errors.is_empty(), "{verb:?}: {errors:#?}\n{doc}");
        assert!(iris_apply::undo(&b.store, b.ledger_of(a), now()).is_err());
    }
}

/// An item that changed after the entry stays as it is: the undo does not
/// reverse a later change.
#[test]
fn the_undo_of_a_state_entry_keeps_a_later_change() {
    let b = setup("undo-later", "open", None);
    let a = b.approved(Verb::Defer, json!({ "date": "2099-01-05" }));
    assert!(matches!(apply_state(&b.store, a), Ok(Outcome::Applied(_))));
    // A later close of the same commitment.
    let later = b.approved(Verb::Close, json!({ "reason": "Done." }));
    assert!(matches!(
        apply_state(&b.store, later),
        Ok(Outcome::Applied(_))
    ));
    let after = b.plain();
    let (_, outcome) = b.undo(a);
    let Outcome::Applied(counts) = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!((counts.changed, counts.unchanged), (0, 1));
    assert_eq!(b.plain(), after, "the later close stays");
    assert_eq!(
        b.one("SELECT count(*) FROM correction WHERE by = 'cs-2'"),
        json!(1),
        "the corrections of the later action stay"
    );
    // A failed entry has no undo.
    let failed = b.approved(Verb::Close, json!({ "reason": "Again." }));
    assert!(matches!(
        apply_state(&b.store, failed),
        Ok(Outcome::Failed { .. })
    ));
    assert!(iris_apply::undo(&b.store, b.ledger_of(failed), now()).is_err());
}

/// Only an undo makes `restore`: submit denies it from any other path.
#[test]
fn submit_denies_restore_with_no_undo_link() {
    let b = setup("restore-denied", "done", None);
    for kind in [AuthorKind::Person, AuthorKind::Agent] {
        let (id, _) = b.submitted(kind, Verb::Restore, json!({}));
        assert_eq!(b.state(id), "denied");
        let reason = b.one(&format!(
            "SELECT reason FROM changeset_action WHERE id = {id}"
        ));
        assert!(reason.as_str().unwrap().contains("undo"), "{reason}");
    }
    assert_eq!(b.one("SELECT status FROM item WHERE id = 1"), json!("done"));
}

/// The link of an undo to a state entry (migration 24) takes only `restore`
/// with no arguments, as step 1.
#[test]
fn the_link_of_a_state_undo_takes_only_restore() {
    let b = setup("restore-link", "open", None);
    let a = b.approved(Verb::Close, json!({ "reason": "Done." }));
    assert!(matches!(apply_state(&b.store, a), Ok(Outcome::Applied(_))));
    let entry = b.ledger_of(a);
    let link = |verb: &str, args: &str, step: i64| {
        b.store.write(|tx| {
            tx.execute(
                "INSERT INTO changeset(author_kind, created_at) VALUES ('person', 0)",
                [],
            )?;
            let cs = tx.last_insert_rowid();
            tx.execute(
                "INSERT INTO changeset_action(changeset_id, revision, n, verb, targets, args)
                 VALUES (?1, 1, 1, ?2, ?3, ?4)",
                rusqlite::params![
                    cs,
                    verb,
                    json!({ "ledger": [format!("l-{entry}")] }).to_string(),
                    args
                ],
            )?;
            tx.execute(
                "INSERT INTO ledger_undo(action_id, ledger_id, step) VALUES (?1, ?2, ?3)",
                rusqlite::params![tx.last_insert_rowid(), entry, step],
            )?;
            Ok(())
        })
    };
    assert!(
        link("restore", r#"{"reason":"x"}"#, 1).is_err(),
        "arguments"
    );
    assert!(link("untrash", "{}", 1).is_err(), "another verb");
    assert!(link("restore", "{}", 2).is_err(), "step 2");
    assert!(link("restore", "{}", 1).is_ok());
}
