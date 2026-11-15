//! Runs the quality test set with a trivial fake understanding function, so
//! that local CI reports the scores with no model. R1-10 plugs in the real
//! understanding function later.
//!
//! The report goes to stderr and to `report.txt` and `report.json` in the
//! Cargo test folder (`target/tmp/iris-eval`).

use iris_eval::{run, sample, Answer};
use iris_synth::{Form, Mailbox, Sender, OWNER};

/// Guesses from the `From` and `Subject` headers only. Mail text is data:
/// this function only compares bytes, and it follows nothing in the text.
fn fake(raw: &[u8]) -> Answer {
    let text = String::from_utf8_lossy(raw);
    let head = text.split("\r\n\r\n").next().unwrap_or("");
    let header = |name: &str| {
        head.split("\r\n")
            .find_map(|l| l.strip_prefix(name))
            .unwrap_or("")
            .to_string()
    };
    let from = header("From: ");
    let subject = header("Subject: ");
    let sender = if from.contains("-secure.") {
        Sender::Hostile
    } else if from.contains(OWNER) {
        Sender::Owner
    } else if from.contains("no-reply@") {
        Sender::KnownService
    } else {
        Sender::KnownPerson
    };
    let form = match sender {
        Sender::Hostile => None,
        _ if subject.contains("receipt") => Some(Form::Record),
        _ if subject.contains("weekly") => Some(Form::Feed),
        _ if subject.contains("sign-in") => Some(Form::Signal),
        _ if subject.contains("Can you send") => Some(Form::Commitment),
        _ => Some(Form::Conversation),
    };
    Answer {
        form,
        sender: Some(sender),
        fields: Default::default(),
    }
}

#[test]
fn fake_understanding_gives_a_report() {
    let mb = Mailbox::new(1, 20_000);
    let report = run(&mb, &sample(&mb, 2_000), fake);
    assert_eq!(report.messages, 2_000);
    assert!(report.form_accuracy().unwrap() > 0.5);
    // The fake gives no fields, so the field recall targets fail.
    assert!(report.failures().iter().any(|f| f.starts_with("field ")));

    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("iris-eval");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("report.txt"), report.text()).unwrap();
    let json = serde_json::to_string_pretty(&report.json()).unwrap();
    std::fs::write(dir.join("report.json"), json).unwrap();
    // Write past the test output capture, so that the local CI log shows it.
    std::io::Write::write_all(&mut std::io::stderr(), report.text().as_bytes()).unwrap();
}
