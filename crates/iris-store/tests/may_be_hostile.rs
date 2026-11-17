//! Each gate that lets the model, a rule, an agent, a notice or a watch use
//! mail reads `may_be_hostile!()`, so a message that waits for its link
//! check counts as hostile mail. This test reads the source of the workspace.
//! A read of the column `hostile` or of the table `link_check` is allowed
//! only in the places below, each with its count and its reason. A new read
//! anywhere fails the test: use `iris_store::may_be_hostile!()`, or add the
//! place here with the reason that it is not a gate.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// Text that reads the column `hostile` of a message (also a field of the
/// same name in Rust), or the table `link_check`.
const READS: [&str; 4] = ["m.hostile", "(hostile", "SET hostile", "FROM link_check"];

/// Each place that may read them, the count of matches of [`READS`] (one
/// condition on the marks gives two), and why.
const ALLOWED: [(&str, usize, &str); 8] = [
    (
        "iris-store/src/lib.rs",
        3,
        "the definition of may_be_hostile",
    ),
    (
        "iris-understand/src/store.rs",
        10,
        "the writer of the marks and the queue of the link check",
    ),
    (
        "iris-api/src/read.rs",
        3,
        "shows the marks, and the count of hostile mail in the brief",
    ),
    (
        "iris-observe/src/lib.rs",
        3,
        "the hostile rule counts marked mail; Rule::query leaves out each queued message",
    ),
    (
        "iris-people/src/lib.rs",
        2,
        "the counts of a profile; no gate reads a profile for the kind of a sender",
    ),
    (
        "iris-talk/src/lib.rs",
        3,
        "the conversation item; the workspace gate reads may_be_hostile",
    ),
    (
        "iris-policy/src/propose.rs",
        1,
        "Message::hostile of the policy facts, which read may_be_hostile",
    ),
    ("iris/src/verbs.rs", 2, "shows the marks of a message"),
];

fn sources(dir: &Path, out: &mut Vec<(String, String)>) {
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n != "tests" && n != "target") {
                sources(&p, out);
            }
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push((
                p.display().to_string().replace('\\', "/"),
                fs::read_to_string(&p).unwrap(),
            ));
        }
    }
}

#[test]
fn each_gate_reads_may_be_hostile() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    sources(&crates, &mut files);
    assert!(
        files.len() > 50,
        "the test reads the workspace: {}",
        files.len()
    );
    let mut got = BTreeMap::new();
    for (path, text) in &files {
        let n: usize = READS.iter().map(|r| text.matches(r).count()).sum();
        if n > 0 {
            got.insert(path.clone(), n);
        }
    }
    let mut bad = Vec::new();
    for (path, n) in &got {
        match ALLOWED
            .iter()
            .find(|(f, _, _)| path.ends_with(&format!("/{f}")))
        {
            Some((_, want, _)) if want == n => {}
            Some((f, want, why)) => bad.push(format!("{f}: {n} reads, {want} allowed ({why})")),
            None => bad.push(format!("{path}: {n} reads, none allowed")),
        }
    }
    for (f, want, why) in ALLOWED {
        if want > 0 && !got.keys().any(|p| p.ends_with(&format!("/{f}"))) {
            bad.push(format!(
                "{f}: no reads, {want} allowed ({why}): change the list"
            ));
        }
    }
    assert!(
        bad.is_empty(),
        "use iris_store::may_be_hostile!():\n{}",
        bad.join("\n")
    );
}
