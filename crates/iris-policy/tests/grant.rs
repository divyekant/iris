//! Tests of a grant inside one job (`docs/specs/2026-11-17-start-job.md`,
//! section 2): the change writes one rule with `job` and keeps the file.

use iris_policy::*;
use iris_store::Store;
use std::path::PathBuf;

const NOW: i64 = 1_800_000_000;

struct Dir(PathBuf);
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn store(name: &str) -> (Dir, Store) {
    let dir = std::env::temp_dir().join(format!("iris-grant-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    (Dir(dir), store)
}

fn version(store: &Store) -> Option<i64> {
    store.read(active).unwrap().version
}

fn grant(job: &str, verbs: &[Verb]) -> Change {
    Change::Grant {
        job: job.into(),
        verbs: verbs.to_vec(),
    }
}

#[test]
fn a_grant_adds_one_rule_with_the_job_and_keeps_the_file() {
    let (dir, store) = store("adds");
    // The first change writes the default file, with its comments.
    change(&store, None, Change::Mode(Mode::Apply), NOW).unwrap();
    let before = std::fs::read_to_string(dir.0.join(FILE)).unwrap();

    let verbs = [Verb::Archive, Verb::Label, Verb::Close];
    let edited = change(&store, version(&store), grant("j-12", &verbs), NOW).unwrap();
    let Edited::Done { policy, .. } = edited else {
        panic!("grant: {edited:?}")
    };
    let text = std::fs::read_to_string(dir.0.join(FILE)).unwrap();
    assert!(text.starts_with(&before), "the old text stays:\n{text}");
    assert!(
        text.contains("# You gave this grant inside job j-12, on the job page."),
        "{text}"
    );

    // The file gives back exactly the rule of the grant.
    assert_eq!(policy.rules.len(), 1);
    let mut want = grant_rule(1, "j-12", &verbs, "2027-01-15");
    want.line = policy.rules[0].line;
    assert_eq!(policy.rules[0], want);
    assert_eq!(
        want.name,
        "In job j-12, allow archive, label and close without review"
    );
    assert_eq!(parse(&text).unwrap(), policy);

    // A second grant takes the next ID.
    let edited = change(&store, version(&store), grant("j-3", &[Verb::Read]), NOW).unwrap();
    let Edited::Done { policy, .. } = edited else {
        panic!("second grant: {edited:?}")
    };
    assert_eq!(policy.rules[1].id, 2);
    assert_eq!(
        policy.rules[1].name,
        "In job j-3, allow read without review"
    );
}

#[test]
fn a_grant_allows_only_the_agent_of_its_job() {
    let (_dir, store) = store("allows");
    let edited = change(&store, None, grant("j-12", &[Verb::Archive]), NOW).unwrap();
    let Edited::Done { policy, .. } = edited else {
        panic!("grant: {edited:?}")
    };
    let msg = Message {
        sender: "dana@example.com",
        sender_kind: SenderKind::Person,
        hostile: false,
        labels: &[],
        date: NOW - 86_400,
        items: &[],
    };
    let decide = |verb, author, job| {
        let a = Action {
            verb,
            author,
            job,
            bulk: false,
            now: NOW,
        };
        policy.decide(&a, [msg]).outcome
    };
    assert_eq!(
        decide(Verb::Archive, Author::Agent, Some("j-12")),
        Outcome::Allow
    );
    // Another job, no job, another verb, another author: the default review.
    assert_eq!(
        decide(Verb::Archive, Author::Agent, Some("j-1")),
        Outcome::Review
    );
    assert_eq!(
        decide(Verb::Archive, Author::Agent, Some("j-120")),
        Outcome::Review
    );
    assert_eq!(decide(Verb::Archive, Author::Agent, None), Outcome::Review);
    assert_eq!(
        decide(Verb::Trash, Author::Agent, Some("j-12")),
        Outcome::Review
    );
    assert_eq!(
        decide(Verb::Archive, Author::Local, Some("j-12")),
        Outcome::Review
    );
}

#[test]
fn a_grant_needs_the_version_that_the_person_saw() {
    let (dir, store) = store("stale");
    change(&store, None, Change::Mode(Mode::Apply), NOW).unwrap();
    let text = std::fs::read_to_string(dir.0.join(FILE)).unwrap();
    assert_eq!(
        change(&store, None, grant("j-1", &[Verb::Archive]), NOW).unwrap(),
        Edited::Stale
    );
    assert_eq!(std::fs::read_to_string(dir.0.join(FILE)).unwrap(), text);
}
