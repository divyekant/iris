//! Each query that skips mail that is not received mail reads
//! `draft_or_copy!()`, so the IMAP labels `UNSENT` and `SENT_COPY` count as
//! `DRAFT` does (docs/specs/2026-11-17-imap-provider.md, section 4). This test
//! reads the source of the workspace. A name of the label `DRAFT`, `UNSENT`
//! or `SENT_COPY` is allowed only in the places below, each with its count
//! and its reason. A new name anywhere fails the test: use
//! `iris_store::draft_or_copy!()`, or add the place here with the reason.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

/// The labels as a string in SQL or in Rust.
const READS: [&str; 6] = [
    "'DRAFT'",
    "\"DRAFT\"",
    "'UNSENT'",
    "\"UNSENT\"",
    "'SENT_COPY'",
    "\"SENT_COPY\"",
];

/// Each place that may name them, the count of matches of [`READS`], and why.
const ALLOWED: [(&str, usize, &str); 16] = [
    (
        "iris-store/src/lib.rs",
        6,
        "the definition of draft_or_copy and DRAFT_OR_COPY (the triggers of migration 37 clear understood_at at a change of SENT or DRAFT)",
    ),
    (
        "iris-understand/src/store.rs",
        1,
        "proof of authorship: your own mail gets no link mark",
    ),
    (
        "iris-senders/src/lib.rs",
        3,
        "the sender axis: SENT and DRAFT are proof of authorship, UNSENT and SENT_COPY are not; received!() uses draft_or_copy",
    ),
    (
        "iris-policy/src/attention.rs",
        1,
        "proof of authorship: mail from you",
    ),
    (
        "iris-job/src/lib.rs",
        1,
        "proof of authorship: a job that read mail you did not send gets the mark; a copy marks it",
    ),
    (
        "iris-changeset/src/flags.rs",
        1,
        "proof of authorship: a phrase of a sent copy counts as external text, the safe side",
    ),
    (
        "iris-changeset/src/lib.rs",
        1,
        "the system labels that a label action cannot target",
    ),
    (
        "iris-talk/src/lib.rs",
        1,
        "proof of authorship: your addresses come from SENT mail that is not DRAFT",
    ),
    (
        "iris-voice/src/lib.rs",
        1,
        "proof of authorship: the samples of your style are SENT mail that is not DRAFT",
    ),
    ("iris-voice/src/tests.rs", 10, "test data"),
    ("iris-fakegmail/src/lib.rs", 5, "the fake Gmail server"),
    ("iris-fakeimap/src/lib.rs", 1, "the fake IMAP server: SEARCH DRAFT"),
    ("iris-gmail/src/raw.rs", 1, "a test of the Gmail client"),
    ("iris-imap/src/lib.rs", 1, "the IMAP client: SEARCH DRAFT"),
    (
        "iris-imap/src/provider.rs",
        3,
        "the IMAP provider writes UNSENT and SENT_COPY, and keeps the fixed label names",
    ),
    (
        "iris-graph/src/provider.rs",
        7,
        "the Outlook provider maps the Graph drafts folder to DRAFT, saves a new draft there, and gives SENT only to the copies that Iris sent",
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
            out.push((
                p.display().to_string().replace('\\', "/"),
                fs::read_to_string(&p).unwrap(),
            ));
        }
    }
}

#[test]
fn each_draft_read_is_classified() {
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
            Some((f, want, why)) => bad.push(format!("{f}: {n} names, {want} allowed ({why})")),
            None => bad.push(format!("{path}: {n} names, none allowed")),
        }
    }
    for (f, want, why) in ALLOWED {
        if want > 0 && !got.keys().any(|p| p.ends_with(&format!("/{f}"))) {
            bad.push(format!(
                "{f}: no names, {want} allowed ({why}): change the list"
            ));
        }
    }
    assert!(
        bad.is_empty(),
        "use iris_store::draft_or_copy!():\n{}",
        bad.join("\n")
    );
}

/// The files with a gate of received mail (a read of `may_be_hostile!`) that
/// do not read `draft_or_copy!` themselves, and why.
const GATES_WITHOUT: [(&str, &str); 3] = [
    (
        "iris-store/src/lib.rs",
        "the definitions of may_be_hostile and draft_or_copy",
    ),
    (
        "iris-policy/src/attention.rs",
        "the facts of one message for its items; items come only from understood mail",
    ),
    (
        "iris-workspace/src/lib.rs",
        "readable() reads iris_talk::message, which counts a draft or a copy as not clean",
    ),
];

/// Each gate of received mail also skips drafts and sent copies: a file
/// that reads `may_be_hostile!` reads `draft_or_copy!` too, or is in
/// [`GATES_WITHOUT`] with its reason.
#[test]
fn each_gate_skips_drafts_and_sent_copies() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut files = Vec::new();
    sources(&crates, &mut files);
    let mut bad = Vec::new();
    for (path, text) in &files {
        let gate = text.contains("may_be_hostile!(") && !path.contains("/tests");
        let skip = text.contains("draft_or_copy!(");
        let listed = GATES_WITHOUT
            .iter()
            .any(|(f, _)| path.ends_with(&format!("/{f}")));
        if gate && !skip && !listed {
            bad.push(path.clone());
        }
        if listed && (!gate || skip) && !path.ends_with("/iris-store/src/lib.rs") {
            bad.push(format!(
                "{path}: listed, but it is no gate without the helper"
            ));
        }
    }
    assert!(
        bad.is_empty(),
        "use iris_store::draft_or_copy!() in each gate:\n{}",
        bad.join("\n")
    );
}
