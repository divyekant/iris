//! Tests of spec 2 section 6.3: the active policy and the confirmation.

use iris_policy::*;
use iris_store::Store;
use std::path::PathBuf;

struct Dir(PathBuf);
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn store(name: &str) -> (Dir, Store) {
    let dir = std::env::temp_dir().join(format!("iris-policy-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    (Dir(dir), store)
}

fn file(ids: &[u32]) -> String {
    let mut s = "version = 0\nmode = \"apply\"\n".to_string();
    for id in ids {
        s += &format!("[[rule]]\nid = {id}\nname = \"r{id}\"\neffect = \"allow\"\nverbs = [\"archive\"]\nwhen = {{ form = \"feed\" }}\nby = \"person\"\nadded = 2026-11-10\n");
    }
    s
}

fn write(store: &Store, text: &str) {
    std::fs::write(store.dir().join(FILE), text).unwrap();
}

fn changed(store: &Store) -> Loaded {
    match file_state(store).unwrap() {
        FileState::Changed(l) => l,
        other => panic!("not changed: {other:?}"),
    }
}

#[test]
fn the_first_start_is_observe_with_no_rules() {
    let (_d, s) = store("first");
    let a = s.read(active).unwrap();
    assert_eq!(
        a,
        Active {
            policy: Policy::default(),
            sha256: None,
            error: None
        }
    );
    assert_eq!(file_state(&s).unwrap(), FileState::Missing);
    assert_eq!(confirm(&s, &sha256_hex(""), 1).unwrap(), Confirm::Missing);
}

#[test]
fn a_file_on_disk_is_not_active_before_the_confirmation() {
    // Requirement C6 of spec 2.
    let (_d, s) = store("c6");
    write(&s, &file(&[1]));
    let seen = changed(&s);
    assert_eq!(s.read(active).unwrap().policy, Policy::default());

    assert_eq!(
        confirm(&s, &seen.sha256, 10).unwrap(),
        Confirm::Done(seen.policy.clone())
    );
    let a = s.read(active).unwrap();
    assert_eq!((a.policy, a.sha256), (seen.policy, Some(seen.sha256)));
    assert_eq!(file_state(&s).unwrap(), FileState::Same);

    // A new rule on disk does not apply before the next confirmation.
    write(&s, &file(&[1, 2]));
    assert_eq!(s.read(active).unwrap().policy.rules.len(), 1);
    assert_eq!(changed(&s).policy.rules.len(), 2);
}

#[test]
fn a_file_that_changed_after_the_view_is_refused() {
    let (_d, s) = store("stale");
    write(&s, &file(&[1]));
    let seen = changed(&s);
    write(&s, &file(&[1, 2]));
    assert_eq!(confirm(&s, &seen.sha256, 10).unwrap(), Confirm::Stale);
    assert_eq!(s.read(active).unwrap().policy, Policy::default());
}

#[test]
fn a_file_that_is_not_valid_keeps_the_active_policy() {
    let (_d, s) = store("invalid");
    write(&s, &file(&[1]));
    let seen = changed(&s);
    confirm(&s, &seen.sha256, 10).unwrap();

    let bad = file(&[1]).replace("form = \"feed\"", "from = \"feed\"");
    write(&s, &bad);
    let FileState::Invalid(e) = file_state(&s).unwrap() else {
        panic!("must be invalid")
    };
    assert!(e.line > 0, "{e}");
    assert!(matches!(
        confirm(&s, &sha256_hex(&bad), 11).unwrap(),
        Confirm::Invalid(_)
    ));
    assert_eq!(s.read(active).unwrap().sha256, Some(seen.sha256.clone()));

    // A missing file keeps the active policy too.
    std::fs::remove_file(s.dir().join(FILE)).unwrap();
    assert_eq!(file_state(&s).unwrap(), FileState::Missing);
    assert_eq!(s.read(active).unwrap().sha256, Some(seen.sha256));
}

#[test]
fn a_removed_rule_id_is_never_used_again() {
    let (_d, s) = store("ids");
    for (ids, ok) in [
        (&[1, 2][..], true),
        (&[1][..], true),
        (&[1, 3][..], true),
        (&[1, 2][..], false),
        (&[3][..], true),
        (&[1][..], false),
    ] {
        write(&s, &file(ids));
        let seen = changed(&s);
        match confirm(&s, &seen.sha256, 10).unwrap() {
            Confirm::Done(_) => assert!(ok, "{ids:?} must be refused"),
            Confirm::Invalid(e) => {
                assert!(!ok, "{ids:?}: {e}");
                assert!(e.message.contains("removed rule"), "{e}");
                assert_eq!(file(ids).lines().nth(e.line - 1), Some("[[rule]]"));
            }
            other => panic!("{other:?}"),
        }
    }
    let rules: Vec<u32> = s
        .read(active)
        .unwrap()
        .policy
        .rules
        .iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(rules, vec![3]);
}

#[test]
fn an_active_text_that_is_no_longer_valid_applies_nothing() {
    let (_d, s) = store("stricter");
    s.write(|tx| {
        tx.execute("INSERT INTO policy_active (id, text, sha256, confirmed_at) VALUES (1, 'version = 9', 'x', 1)", [])?;
        Ok(())
    })
    .unwrap();
    let a = s.read(active).unwrap();
    assert_eq!(a.policy, Policy::default());
    assert!(a.error.is_some());
}
