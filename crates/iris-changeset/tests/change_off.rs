//! The switch of a change of a draft (docs/specs/2026-11-25-draft-change.md).
//! Its own test binary: the switch is global to the process.

use iris_changeset::{add_action, create, submit, Author, AuthorKind, Verb};
use iris_store::Store;
use serde_json::json;

/// Off by default: submit denies each change of a draft, so no approved
/// change reaches an apply that cannot do it.
#[test]
fn submit_denies_a_change_while_the_switch_is_off() {
    let dir = std::env::temp_dir().join(format!("iris-cw-off-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let s = Store::open(&dir).unwrap();
    s.add_account("gmail", "owner@example.com").unwrap();
    let who = Author {
        kind: AuthorKind::Person,
        name: Some("t".into()),
        session: None,
        job: None,
    };
    let cs = create(&s, &who, 1).unwrap();
    let args = json!({"to": ["priya@example.com"], "body": "Thanks"});
    add_action(
        &s,
        cs,
        &who,
        Verb::Draft,
        &["d-1"],
        args.as_object().unwrap(),
        None,
    )
    .unwrap();
    submit(&s, cs, &who, "Write.", 1).unwrap();
    let got: (String, Option<String>) = s
        .read(|c| {
            Ok(
                c.query_row("SELECT state, reason FROM changeset_action", [], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })?,
            )
        })
        .unwrap();
    assert_eq!(
        got,
        ("denied".into(), Some(iris_changeset::CHANGE_OFF.into()))
    );
    let _ = std::fs::remove_dir_all(&dir);
}
