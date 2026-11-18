//! Tests of the changes from the web client (spec 2 section 6.3): the engine
//! writes `policy.toml`, keeps the other text and comments, and makes the
//! new text active in one step.

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
    let dir = std::env::temp_dir().join(format!("iris-edit-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    (Dir(dir), store)
}

fn disk(store: &Store) -> String {
    std::fs::read_to_string(store.dir().join(FILE)).unwrap()
}

fn rule(id: u32) -> String {
    format!("[[rule]]\nid = {id}\nname = \"r{id}\"\neffect = \"hide\"\nwhen = {{ form = \"feed\" }}\nby = \"person\"\nadded = 2026-11-10\n")
}

/// The rules without their lines, so that two policies compare by content.
fn rules(p: &Policy) -> Vec<Rule> {
    p.rules
        .iter()
        .map(|r| Rule {
            line: 0,
            ..r.clone()
        })
        .collect()
}

/// A small deterministic random source, so that a failure repeats.
struct Rng(u64);
impl Rng {
    fn next(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }
}

/// A valid policy text with comments and blank lines in random places.
/// Each comment names the rule that follows it, or says "all" when it is
/// separated from that rule by a blank line.
fn random_file(rng: &mut Rng, n: u32) -> String {
    let mut s = "# head\nversion = 0\nmode = \"apply\" # the mode\n".to_string();
    for id in 1..=n {
        if rng.next(2) == 0 {
            s += &format!("\n# all before {id}\n\n");
        }
        if rng.next(2) == 0 {
            s += "\n";
        }
        if rng.next(2) == 0 {
            s += &format!("# about {id}\n");
        }
        s += &rule(id).replace("name =", &format!("# inside {id}\nname ="));
    }
    if rng.next(2) == 0 {
        s += "\n# tail\n";
    }
    s
}

#[test]
fn the_default_text_is_the_policy_of_the_first_start() {
    assert_eq!(parse(DEFAULT_TEXT).unwrap(), Policy::default());
}

#[test]
fn a_removal_keeps_each_other_rule_and_each_comment_of_the_others() {
    // Invariant: a removal changes only the removed rule and the comments
    // directly above it. Each other rule and each other comment stays.
    let mut rng = Rng(0x9e3779b97f4a7c15);
    for case in 0..300 {
        let n = 1 + rng.next(5) as u32;
        let text = random_file(&mut rng, n);
        let before = parse(&text).unwrap();
        let id = 1 + rng.next(n as usize) as u32;
        let out = edit_text(&text, Change::Remove(id)).unwrap().unwrap();
        let after = parse(&out).unwrap_or_else(|e| panic!("case {case}: {e}\n{out}"));
        let mut want = rules(&before);
        want.retain(|r| r.id != id);
        assert_eq!(rules(&after), want, "case {case}\n{text}\n---\n{out}");
        assert_eq!(after.mode, before.mode);
        // A shared comment keeps the blank line that separates it.
        let lines: Vec<&str> = out.lines().collect();
        for (i, l) in lines.iter().enumerate() {
            if l.starts_with("# all before") {
                assert_eq!(
                    lines.get(i + 1),
                    Some(&""),
                    "case {case}\n{text}\n---\n{out}"
                );
            }
        }
        for line in text.lines().filter(|l| l.starts_with('#')) {
            let gone = *line == format!("# about {id}") || *line == format!("# inside {id}");
            assert_eq!(
                out.contains(line),
                !gone,
                "case {case}: {line}\n{text}\n---\n{out}"
            );
        }
    }
}

/// The same rules as an array of inline tables, which the parser accepts
/// too (issue 144), with a comment above some entries.
fn random_inline(rng: &mut Rng, n: u32) -> String {
    let mut s = "# head\nversion = 0\nmode = \"apply\"\nrule = [\n".to_string();
    for id in 1..=n {
        if rng.next(2) == 0 {
            s += &format!("  # all before {id}\n\n");
        }
        if rng.next(2) == 0 {
            s += &format!("  # about {id}\n");
        }
        s += &format!(
            "  {{ id = {id}, name = \"r{id}\", effect = \"hide\", when = {{ form = \"feed\" }}, by = \"person\", added = 2026-11-10 }},\n"
        );
    }
    if rng.next(2) == 0 {
        s += "  # footer\n";
    }
    s + "]\n# tail\n"
}

#[test]
fn a_removal_finds_a_rule_in_an_inline_array() {
    // Invariant: a removal works on each layout of the rules that the
    // parser accepts, and changes only the removed rule.
    let mut rng = Rng(0x2545f4914f6cdd1d);
    for case in 0..200 {
        let n = 1 + rng.next(5) as u32;
        let text = random_inline(&mut rng, n);
        let before = parse(&text).unwrap_or_else(|e| panic!("case {case}: {e}\n{text}"));
        let id = 1 + rng.next(n as usize) as u32;
        let out = edit_text(&text, Change::Remove(id))
            .unwrap()
            .unwrap_or_else(|| panic!("case {case}: rule {id} not found\n{text}"));
        let after = parse(&out).unwrap_or_else(|e| panic!("case {case}: {e}\n{out}"));
        let mut want = rules(&before);
        want.retain(|r| r.id != id);
        assert_eq!(rules(&after), want, "case {case}\n{text}\n---\n{out}");
        for line in text.lines().filter(|l| l.trim_start().starts_with('#')) {
            let gone = line.trim() == format!("# about {id}");
            assert_eq!(out.contains(line), !gone, "case {case}: {line}\n---\n{out}");
        }
        assert_eq!(edit_text(&text, Change::Remove(n + 1)), Ok(None));
    }
}

#[test]
fn a_grant_adds_a_rule_to_an_inline_array() {
    let (_dir, s) = store("inline-add");
    let text = random_inline(&mut Rng(3), 2);
    std::fs::write(s.dir().join(FILE), &text).unwrap();
    let FileState::Changed(l) = file_state(&s).unwrap() else {
        panic!("the file is a change")
    };
    let Confirm::Done { version, .. } = confirm(&s, &l.sha256, 1).unwrap() else {
        panic!("confirm")
    };
    let got = change(
        &s,
        Some(version),
        Change::Grant {
            job: "j-4".into(),
            verbs: vec![Verb::Archive],
        },
        2,
    )
    .unwrap();
    let Edited::Done { policy, .. } = got else {
        panic!("grant: {got:?}")
    };
    let out = disk(&s);
    assert_eq!(parse(&out).unwrap(), policy, "{out}");
    assert_eq!(policy.rules.len(), 3, "{out}");
    assert_eq!(policy.rules[2].job.as_deref(), Some("j-4"));
    assert!(
        out.contains("# You gave this grant inside job j-4"),
        "{out}"
    );
    assert!(out.ends_with("]\n# tail\n"), "{out}");
}

#[test]
fn a_removal_keeps_the_blank_line_after_a_shared_comment() {
    let text = format!("# shared\n\n# about 1\n{}{}", rule(1), rule(2));
    let out = edit_text(&text, Change::Remove(1)).unwrap().unwrap();
    assert_eq!(out, format!("# shared\n\n{}", rule(2)));
    let text = format!("{}\n# shared\n\n# about 2\n{}", rule(1), rule(2));
    let out = edit_text(&text, Change::Remove(2)).unwrap().unwrap();
    assert_eq!(out, format!("{}\n# shared\n\n", rule(1)));
}

#[test]
fn a_mode_change_keeps_each_comment_and_each_rule() {
    let mut rng = Rng(7);
    for _ in 0..50 {
        let n = rng.next(4) as u32;
        let text = random_file(&mut rng, n);
        let out = edit_text(&text, Change::Mode(Mode::Observe))
            .unwrap()
            .unwrap();
        assert_eq!(out, text.replace("mode = \"apply\"", "mode = \"observe\""));
        assert_eq!(parse(&out).unwrap().mode, Mode::Observe);
    }
}

#[test]
fn a_removal_of_an_unknown_id_changes_nothing() {
    assert_eq!(edit_text(&rule(1), Change::Remove(2)), Ok(None));
    assert_eq!(edit_text(DEFAULT_TEXT, Change::Remove(1)), Ok(None));
}

#[test]
fn a_change_at_the_first_start_writes_the_file_and_makes_it_active() {
    let (_d, s) = store("first");
    let got = change(&s, None, Change::Mode(Mode::Apply), 10).unwrap();
    let Edited::Done { version, policy } = got else {
        panic!("{got:?}")
    };
    assert_eq!((version, policy.mode), (1, Mode::Apply));
    assert_eq!(disk(&s), DEFAULT_TEXT.replace("\"observe\"", "\"apply\""));
    assert_eq!(file_state(&s).unwrap(), FileState::Same);
    let a = s.read(active).unwrap();
    assert_eq!((a.version, a.policy.mode), (Some(1), Mode::Apply));
}

#[test]
fn a_removed_rule_leaves_the_file_and_its_id_is_never_used_again() {
    let (_d, s) = store("remove");
    let text = format!("version = 0\nmode = \"apply\"\n{}{}", rule(1), rule(2));
    std::fs::write(s.dir().join(FILE), &text).unwrap();
    assert!(matches!(
        confirm(&s, &sha256_hex(&text), 1).unwrap(),
        Confirm::Done { version: 1, .. }
    ));

    let got = change(&s, Some(1), Change::Remove(1), 2).unwrap();
    let Edited::Done { version: 2, policy } = got else {
        panic!("{got:?}")
    };
    assert_eq!(policy.rules.iter().map(|r| r.id).collect::<Vec<_>>(), [2]);
    assert_eq!(file_state(&s).unwrap(), FileState::Same);

    // A file that brings the ID back cannot be confirmed.
    let back = format!("{}{}", disk(&s), rule(1));
    std::fs::write(s.dir().join(FILE), &back).unwrap();
    assert!(matches!(file_state(&s).unwrap(), FileState::Invalid(_)));
}

#[test]
fn a_change_needs_the_active_version_that_the_person_saw() {
    let (_d, s) = store("stale");
    assert!(matches!(
        change(&s, None, Change::Mode(Mode::Apply), 1).unwrap(),
        Edited::Done { .. }
    ));
    // The person saw the policy of the first start, and version 1 is active now.
    assert_eq!(
        change(&s, None, Change::Mode(Mode::Observe), 2).unwrap(),
        Edited::Stale
    );
    assert_eq!(
        change(&s, Some(1), Change::Remove(9), 2).unwrap(),
        Edited::NoRule
    );
    assert_eq!(s.read(active).unwrap().version, Some(1));
}

#[test]
fn a_change_waits_while_the_file_is_not_the_active_policy() {
    // Spec 2 section 6.3: the person first confirms or discards the file.
    let (_d, s) = store("waits");
    let path = s.dir().join(FILE);
    for text in [
        format!("version = 0\nmode = \"apply\"\n{}", rule(1)),
        "mode = ".to_string(),
    ] {
        std::fs::write(&path, &text).unwrap();
        assert_eq!(
            change(&s, None, Change::Mode(Mode::Apply), 1).unwrap(),
            Edited::FileChanged
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        assert_eq!(s.read(active).unwrap().version, None);
    }
}

#[test]
fn a_change_after_a_confirmation_needs_the_file() {
    // A missing file after a confirmation is not the active policy either.
    let (_d, s) = store("gone");
    let text = format!("version = 0\nmode = \"apply\"\n{}", rule(1));
    std::fs::write(s.dir().join(FILE), &text).unwrap();
    assert!(matches!(
        confirm(&s, &sha256_hex(&text), 1).unwrap(),
        Confirm::Done { .. }
    ));
    std::fs::remove_file(s.dir().join(FILE)).unwrap();
    assert_eq!(
        change(&s, Some(1), Change::Remove(1), 2).unwrap(),
        Edited::FileChanged
    );
    assert!(!s.dir().join(FILE).exists());
    assert_eq!(s.read(active).unwrap().version, Some(1));
}

fn saved(d: Discarded) -> String {
    match d {
        Discarded::Done { saved: Some(p) } => std::fs::read_to_string(p).unwrap(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_discard_keeps_a_copy_and_writes_the_active_text() {
    let (_d, s) = store("discard");
    let path = s.dir().join(FILE);
    // Nothing to discard at the first start with no file.
    assert_eq!(discard(&s).unwrap(), Discarded::Nothing);
    assert!(!path.exists());

    // A file at the first start: the discard leaves no file, and keeps a copy.
    let edit = format!("version = 0\nmode = \"apply\"\n{}", rule(1));
    std::fs::write(&path, &edit).unwrap();
    assert_eq!(saved(discard(&s).unwrap()), edit);
    assert_eq!(file_state(&s).unwrap(), FileState::Missing);
    assert_eq!(s.read(active).unwrap().version, None);

    // A file that is not valid, after a confirmation: the discard writes the active text.
    let text = format!("version = 0\nmode = \"apply\"\n{}", rule(2));
    std::fs::write(&path, &text).unwrap();
    assert!(matches!(
        confirm(&s, &sha256_hex(&text), 1).unwrap(),
        Confirm::Done { .. }
    ));
    for bad in ["mode = ", "version = 0\nmode = \"observe\"\n"] {
        std::fs::write(&path, bad).unwrap();
        assert_eq!(saved(discard(&s).unwrap()), bad);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        assert_eq!(file_state(&s).unwrap(), FileState::Same);
    }
    assert_eq!(discard(&s).unwrap(), Discarded::Nothing);

    // A missing file comes back.
    std::fs::remove_file(&path).unwrap();
    assert_eq!(discard(&s).unwrap(), Discarded::Done { saved: None });
    assert_eq!(file_state(&s).unwrap(), FileState::Same);
}
