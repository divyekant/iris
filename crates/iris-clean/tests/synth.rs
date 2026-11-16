//! The test set: expected clean text of samples from the synthetic mailbox.
//!
//! Each file in `tests/golden/` holds the expected result of one sample. To
//! write the files again after a change on purpose, run the tests with
//! `IRIS_CLEAN_BLESS=1`, then read each changed file before you commit it.

use iris_clean::{clean, Clean};
use iris_synth::{traits, Form, Mailbox, Sender};

const SEED: u64 = 1;
const COUNT: usize = 20_000;

fn render(c: &Clean) -> String {
    let mut s = format!("subject: {}\ncut: {}\n", c.subject, c.cut);
    for a in &c.attachments {
        s.push_str(&format!(
            "attachment: {:?} {} {} bytes, text: {}\n",
            a.name,
            a.mime_type,
            a.size,
            a.text.is_some()
        ));
    }
    s + "---\n" + &c.text + "\n"
}

/// The samples: a name and the first message that matches.
fn samples(mb: &Mailbox) -> Vec<(&'static str, usize)> {
    let raw = |i: usize| String::from_utf8_lossy(&mb.raw(i)).into_owned();
    let first = |f: &dyn Fn(usize) -> bool| (0..mb.len()).find(|&i| f(i)).expect("a sample");
    let plain = |i: usize| mb.truth(i).traits == 0 && mb.truth(i).message_id.contains(".0@");
    let has = |i: usize, t: u8| mb.truth(i).traits & t != 0;
    let mut out = Vec::new();
    for form in Form::ALL {
        out.push((
            form.name(),
            first(&|i| plain(i) && mb.truth(i).form == form),
        ));
    }
    out.push((
        "reply",
        first(&|i| {
            let t = mb.truth(i);
            t.traits == 0 && t.sender == Sender::Owner && !t.message_id.contains(".0@")
        }),
    ));
    out.push(("odd_subject", first(&|i| raw(i).contains("=?UTF-8?B?"))));
    out.push((
        "odd_latin1",
        first(&|i| raw(i).contains("charset=iso-8859-1")),
    ));
    out.push(("odd_qp", first(&|i| raw(i).contains("quoted-printable"))));
    out.push((
        "injection_hidden",
        first(&|i| has(i, traits::PROMPT_INJECTION) && raw(i).contains("display:none")),
    ));
    out.push((
        "injection_visible",
        first(&|i| has(i, traits::PROMPT_INJECTION) && !raw(i).contains("text/html")),
    ));
    out.push(("phishing_link", first(&|i| has(i, traits::PHISHING_LINK))));
    out.push((
        "spoofed_name",
        first(&|i| mb.truth(i).traits == traits::SPOOFED_NAME),
    ));
    out.push(("big_attachment", first(&|i| has(i, traits::BIG_ATTACHMENT))));
    out
}

#[test]
fn samples_agree_with_the_expected_text() {
    let mb = Mailbox::new(SEED, COUNT);
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden");
    let bless = std::env::var_os("IRIS_CLEAN_BLESS").is_some();
    let mut wrong = Vec::new();
    for (name, i) in samples(&mb) {
        let got = render(&clean(&mb.raw(i)));
        let path = format!("{dir}/{name}.txt");
        if bless {
            std::fs::write(&path, &got).unwrap();
            continue;
        }
        let want = std::fs::read_to_string(&path).unwrap_or_default();
        if got != want {
            wrong.push(format!("{name} (message {i}):\n{got}"));
        }
    }
    assert!(
        wrong.is_empty(),
        "not the expected text:\n{}",
        wrong.join("\n")
    );
}

#[test]
fn every_message_of_the_mailbox_is_clean() {
    let mb = Mailbox::new(SEED, COUNT);
    for i in 0..mb.len() {
        let t = mb.truth(i);
        let c = clean(&mb.raw(i));
        assert!(!c.text.is_empty(), "empty text in message {i}");
        assert!(
            !c.subject.is_empty() && !c.subject.contains("=?"),
            "subject of {i}"
        );
        assert!(
            !c.text.contains('\r') && !c.text.contains('<'),
            "markup in {i}"
        );
        assert!(
            !c.text.contains('\u{fffd}'),
            "bad charset in {i}: {}",
            c.text
        );
        if t.traits & traits::PROMPT_INJECTION != 0 {
            let hidden = String::from_utf8_lossy(&mb.raw(i)).contains("display:none");
            // Hidden text goes out. Text that a person sees stays: it is data.
            let attack = ["Ignore all", "SYSTEM:", "Note to the AI"];
            assert_eq!(
                attack.iter().any(|a| c.text.contains(a)),
                !hidden,
                "message {i}"
            );
        }
        if t.traits & traits::PHISHING_LINK != 0 {
            assert!(c.text.contains("-secure.example.net/verify"), "message {i}");
        }
        if t.traits & traits::BIG_ATTACHMENT != 0 {
            let a = &c.attachments[0];
            assert_eq!(a.mime_type, "application/pdf");
            assert!((1_000_000..4_000_000).contains(&a.size), "size {}", a.size);
        } else {
            assert!(c.attachments.is_empty(), "attachments in {i}");
        }
    }
}
