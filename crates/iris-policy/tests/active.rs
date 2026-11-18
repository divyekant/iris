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
            version: None,
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
        Confirm::Done {
            version: 1,
            policy: seen.policy.clone()
        }
    );
    let a = s.read(active).unwrap();
    assert_eq!(
        (a.policy, a.version, a.sha256),
        (seen.policy, Some(1), Some(seen.sha256))
    );
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
    assert_eq!(s.read(next_rule_id).unwrap(), 1);
    let mut seen = vec![];
    for (ids, ok) in [
        (&[7][..], true),
        (&[1, 2][..], true),
        (&[1][..], true),
        (&[1, 3][..], true),
        (&[1, 2][..], false),
        (&[3][..], true),
        (&[1][..], false),
    ] {
        write(&s, &file(ids));
        // The status before the confirmation already names the error.
        if !ok {
            let FileState::Invalid(e) = file_state(&s).unwrap() else {
                panic!("{ids:?} must be invalid")
            };
            assert!(e.message.contains("removed rule"), "{e}");
        }
        match confirm(&s, &sha256_hex(&file(ids)), 10).unwrap() {
            Confirm::Done { .. } => {
                assert!(ok, "{ids:?} must be refused");
                // The next ID is free: no rule had it, also not a removed one.
                seen.extend_from_slice(ids);
                let next = s.read(next_rule_id).unwrap();
                assert_eq!(next, seen.iter().max().unwrap() + 1, "{ids:?}");
            }
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
        tx.execute("INSERT INTO policy_version (text, sha256, confirmed_at) VALUES ('version = 9', 'x', 1)", [])?;
        Ok(())
    })
    .unwrap();
    let a = s.read(active).unwrap();
    assert_eq!(a.policy, Policy::default());
    assert!(a.error.is_some());
}

#[test]
fn each_confirmed_version_keeps_its_rule_text() {
    // A rule keeps its ID when the person edits it. The version and the ID
    // together name one rule text.
    let (_d, s) = store("versions");
    let one = file(&[1]);
    write(&s, &one);
    confirm(&s, &sha256_hex(&one), 10).unwrap();
    let edited = one.replace("form = \"feed\"", "form = \"record\"");
    write(&s, &edited);
    let Confirm::Done { version, .. } = confirm(&s, &sha256_hex(&edited), 11).unwrap() else {
        panic!()
    };
    assert_eq!(version, 2);
    assert_eq!(
        s.read(|c| version_text(c, 1)).unwrap().as_deref(),
        Some(one.as_str())
    );
    assert_eq!(
        s.read(|c| version_text(c, 2)).unwrap().as_deref(),
        Some(edited.as_str())
    );
    assert_eq!(s.read(|c| version_text(c, 3)).unwrap(), None);
    assert_eq!(s.read(active).unwrap().version, Some(2));
}

/// Issue 143: a confirm waits for the backup lock, as a discard and a
/// change do, so a confirm of another process cannot come between the
/// read and the file write of a discard.
#[test]
fn a_confirm_waits_while_another_writer_holds_the_policy_lock() {
    let (_d, s) = store("confirm-lock");
    write(&s, &file(&[1]));
    let seen = changed(&s);
    // A discard or a change of another process holds the lock.
    let lock = s.backup_lock().unwrap();
    let dir = s.dir().to_path_buf();
    let sha = seen.sha256.clone();
    let other = std::thread::spawn(move || {
        let s = Store::open(&dir).unwrap();
        confirm(&s, &sha, 10).unwrap()
    });
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert_eq!(
        s.read(active).unwrap().version,
        None,
        "the confirm did not wait for the lock"
    );
    drop(lock);
    assert!(matches!(other.join().unwrap(), Confirm::Done { .. }));
    assert!(s.read(active).unwrap().version.is_some());
}
