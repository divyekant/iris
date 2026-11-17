//! Each gate that lets the model, a rule, an agent, a notice or a watch use
//! mail reads `may_be_hostile!()`, so a message that waits for its link
//! check counts as hostile mail. This test reads the source of the workspace
//! and finds each other form of the hostile condition.

use std::fs;
use std::path::Path;

/// The other forms of the condition.
const FORMS: [&str; 4] = [
    "hostile IS NULL OR",
    "hostile IS NOT NULL AND",
    "coalesce(m.hostile, '[]') = '[]'",
    "FROM link_check c WHERE",
];

/// Files that may use another form, and why. Each one shows or selects
/// hostile mail; none of them lets anything use the mail.
const ALLOWED: [(&str, &str); 4] = [
    ("iris-store/src/lib.rs", "the definition of may_be_hostile"),
    (
        "iris-observe/src/lib.rs",
        "the hostile rule archives marked mail only",
    ),
    (
        "iris-api/src/read.rs",
        "the count of hostile mail in the brief",
    ),
    (
        "iris-people/src/lib.rs",
        "hostile mail does not count for a person",
    ),
];

fn sources(dir: &Path, out: &mut Vec<(String, String)>) {
    for e in fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            if p.file_name().is_some_and(|n| n != "tests" && n != "target") {
                sources(&p, out);
            }
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push((p.display().to_string(), fs::read_to_string(&p).unwrap()));
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
    let mut bad = Vec::new();
    for (path, text) in &files {
        let path = path.replace('\\', "/");
        if ALLOWED.iter().any(|(f, _)| path.ends_with(f)) {
            continue;
        }
        for form in FORMS {
            if text.contains(form) {
                bad.push(format!("{path}: {form}"));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "use iris_store::may_be_hostile!():\n{}",
        bad.join("\n")
    );
}
