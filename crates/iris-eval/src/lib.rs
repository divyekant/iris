//! The quality test set of Iris (tracker item R1-14).
//!
//! `run` gives each sample message of an `iris_synth::Mailbox` to an
//! understanding function, and compares each answer with the truth of the
//! mailbox. The `Report` gives precision and recall for each form, each
//! sender kind and each field, as text and as JSON. `Report::failures` lists
//! each value under its target.
//!
//! Hostile mail must make no item (spec 1, section 6.7). So the expected form
//! of a hostile message is "no item" (`None`), and `none` is a class of the
//! form scores.

use iris_synth::{Form, Mailbox, Sender};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

// Quality targets. These are a proposal: spec 1 section 10 sets the real
// targets after the first measure, and the lead and dk can change them.

/// Minimum precision and recall for each form (and for `none`).
pub const FORM_TARGET: f64 = 0.90;
/// Minimum recall of `none`: each hostile message must make no item.
pub const NO_ITEM_RECALL_TARGET: f64 = 1.0;
/// Minimum precision and recall for each sender kind. The kind comes from
/// fixed rules (spec 1, section 6.7), so the target is higher.
pub const SENDER_TARGET: f64 = 0.95;
/// Minimum precision and recall for each field of records and commitments.
pub const FIELD_TARGET: f64 = 0.90;

/// The answer of an understanding function for one message.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Answer {
    /// The form of the item. `None` means no item (for example hostile mail).
    pub form: Option<Form>,
    /// The sender kind. `None` means no answer.
    pub sender: Option<Sender>,
    /// Item fields as dotted keys of the open model, the same keys as
    /// `iris_synth::Mailbox::fields`, for example `fields.amount.value`.
    pub fields: BTreeMap<String, String>,
}

/// The true answer for message `i`, from the truth of the mailbox.
pub fn expected(mb: &Mailbox, i: usize) -> Answer {
    let t = mb.truth(i);
    Answer {
        form: (t.sender != Sender::Hostile).then_some(t.form),
        sender: Some(t.sender),
        fields: mb
            .fields(i)
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    }
}

/// `n` message indices, spread evenly over the mailbox, oldest first. If `n`
/// is the size of the mailbox or more, each message is in the sample.
pub fn sample(mb: &Mailbox, n: usize) -> Vec<usize> {
    let len = mb.len();
    if n >= len {
        return (0..len).collect();
    }
    (0..n).map(|j| j * len / n).collect()
}

/// Counts for one class or one field.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Count {
    /// Answers with this class (or this field) that are correct.
    pub correct: usize,
    /// Answers that give this class (or this field).
    pub answered: usize,
    /// Messages whose truth has this class (or this field).
    pub expected: usize,
}

impl Count {
    /// `None` when there is no answer with this class.
    pub fn precision(&self) -> Option<f64> {
        ratio(self.correct, self.answered)
    }

    /// `None` when no message of the sample has this class.
    pub fn recall(&self) -> Option<f64> {
        ratio(self.correct, self.expected)
    }
}

fn ratio(a: usize, b: usize) -> Option<f64> {
    (b > 0).then(|| a as f64 / b as f64)
}

/// The scores of one run.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Report {
    pub messages: usize,
    /// Messages with the correct form (or correctly no item).
    pub form_correct: usize,
    /// Key: a form name, or `none` for no item.
    pub forms: BTreeMap<&'static str, Count>,
    /// Key: a sender kind name.
    pub senders: BTreeMap<&'static str, Count>,
    /// Key: a field key.
    pub fields: BTreeMap<String, Count>,
}

fn form_name(f: Option<Form>) -> &'static str {
    f.map_or("none", Form::name)
}

/// Run `understand` on the raw bytes of each message in `indices`, and score
/// each answer against the truth.
pub fn run(mb: &Mailbox, indices: &[usize], mut understand: impl FnMut(&[u8]) -> Answer) -> Report {
    let mut r = Report::default();
    for name in Form::ALL.map(Form::name).into_iter().chain(["none"]) {
        r.forms.insert(name, Count::default());
    }
    for &i in indices {
        let want = expected(mb, i);
        let got = understand(&mb.raw(i));
        r.messages += 1;

        let ok = got.form == want.form;
        r.form_correct += usize::from(ok);
        r.forms.get_mut(form_name(want.form)).unwrap().expected += 1;
        let c = r.forms.get_mut(form_name(got.form)).unwrap();
        c.answered += 1;
        c.correct += usize::from(ok);

        if let Some(s) = want.sender {
            r.senders.entry(s.name()).or_default().expected += 1;
        }
        if let Some(s) = got.sender {
            let c = r.senders.entry(s.name()).or_default();
            c.answered += 1;
            c.correct += usize::from(got.sender == want.sender);
        }

        for k in want.fields.keys() {
            r.fields.entry(k.clone()).or_default().expected += 1;
        }
        for (k, v) in &got.fields {
            let c = r.fields.entry(k.clone()).or_default();
            c.answered += 1;
            c.correct += usize::from(want.fields.get(k) == Some(v));
        }
    }
    r
}

impl Report {
    /// The share of messages with the correct form.
    pub fn form_accuracy(&self) -> Option<f64> {
        ratio(self.form_correct, self.messages)
    }

    /// Each score under its target, as one line each. Empty when each target
    /// is met. A class with no answer and no message has no score and is
    /// skipped.
    pub fn failures(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut check = |group: &str, name: &str, metric: &str, v: Option<f64>, min: f64| {
            if let Some(v) = v.filter(|v| *v < min) {
                out.push(format!("{group} {name}: {metric} {v:.3} < {min:.3}"));
            }
        };
        for (name, c) in &self.forms {
            check("form", name, "precision", c.precision(), FORM_TARGET);
            let min = if *name == "none" {
                NO_ITEM_RECALL_TARGET
            } else {
                FORM_TARGET
            };
            check("form", name, "recall", c.recall(), min);
        }
        for (name, c) in &self.senders {
            check("sender", name, "precision", c.precision(), SENDER_TARGET);
            check("sender", name, "recall", c.recall(), SENDER_TARGET);
        }
        for (name, c) in &self.fields {
            check("field", name, "precision", c.precision(), FIELD_TARGET);
            check("field", name, "recall", c.recall(), FIELD_TARGET);
        }
        out
    }

    /// A plain-text table of the scores.
    pub fn text(&self) -> String {
        let pct = |v: Option<f64>| v.map_or("-".to_string(), |v| format!("{:.1}%", v * 100.0));
        let mut s = format!(
            "Iris quality test set: {} messages, form accuracy {}\n",
            self.messages,
            pct(self.form_accuracy())
        );
        let groups: [(&str, Vec<(&str, &Count)>); 3] = [
            ("form", self.forms.iter().map(|(k, c)| (*k, c)).collect()),
            (
                "sender",
                self.senders.iter().map(|(k, c)| (*k, c)).collect(),
            ),
            (
                "field",
                self.fields.iter().map(|(k, c)| (k.as_str(), c)).collect(),
            ),
        ];
        for (group, rows) in groups {
            s += &format!(
                "\n{group:<26} {:>9} {:>9} {:>8} {:>8} {:>8}\n",
                "precision", "recall", "correct", "answered", "expected"
            );
            for (name, c) in rows {
                s += &format!(
                    "{name:<26} {:>9} {:>9} {:>8} {:>8} {:>8}\n",
                    pct(c.precision()),
                    pct(c.recall()),
                    c.correct,
                    c.answered,
                    c.expected
                );
            }
        }
        let failures = self.failures();
        s += &format!("\nUnder target: {}\n", failures.len());
        for f in failures {
            s += &format!("  {f}\n");
        }
        s
    }

    /// The scores and the failures as one JSON document. A score with no
    /// value is `null`.
    pub fn json(&self) -> Value {
        fn group<'a>(rows: impl Iterator<Item = (&'a str, &'a Count)>) -> Value {
            let mut m = Map::new();
            for (name, c) in rows {
                m.insert(
                    name.to_string(),
                    json!({
                        "precision": c.precision(),
                        "recall": c.recall(),
                        "correct": c.correct,
                        "answered": c.answered,
                        "expected": c.expected,
                    }),
                );
            }
            Value::Object(m)
        }
        json!({
            "messages": self.messages,
            "form_accuracy": self.form_accuracy(),
            "forms": group(self.forms.iter().map(|(k, c)| (*k, c))),
            "senders": group(self.senders.iter().map(|(k, c)| (*k, c))),
            "fields": group(self.fields.iter().map(|(k, c)| (k.as_str(), c))),
            "targets": {
                "form": FORM_TARGET,
                "no_item_recall": NO_ITEM_RECALL_TARGET,
                "sender": SENDER_TARGET,
                "field": FIELD_TARGET,
            },
            "failures": self.failures(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_perfect_answer_meets_each_target() {
        let mb = Mailbox::new(11, 3000);
        let idx = sample(&mb, 3000);
        let mut i = 0;
        let r = run(&mb, &idx, |_| {
            i += 1;
            expected(&mb, idx[i - 1])
        });
        assert_eq!(r.messages, 3000);
        assert_eq!(r.form_accuracy(), Some(1.0));
        assert!(r.failures().is_empty(), "{:?}", r.failures());
        assert!(r.forms["none"].expected > 0, "the sample has hostile mail");
        assert!(r.fields.contains_key("fields.amount.value"));
        assert!(r.fields.contains_key("due.date"));
    }

    #[test]
    fn scores_are_exact_for_a_known_answer() {
        let mb = Mailbox::new(11, 3000);
        // Find one record and one feed item of a known service, and one
        // hostile message.
        let find =
            |p: &dyn Fn(&Answer) -> bool| (0..mb.len()).find(|i| p(&expected(&mb, *i))).unwrap();
        let service = |a: &Answer, f| a.form == Some(f) && a.sender == Some(Sender::KnownService);
        let rec = find(&|a| service(a, Form::Record));
        let hostile = find(&|a| a.form.is_none());
        let feed = find(&|a| service(a, Form::Feed));
        let mut answers = vec![
            // A record that is called a feed, with a wrong amount and a correct vendor.
            {
                let mut a = expected(&mb, rec);
                a.form = Some(Form::Feed);
                a.fields.insert("fields.amount.value".into(), "0.00".into());
                a.fields.remove("fields.order");
                a
            },
            // Hostile mail that makes a signal.
            Answer {
                form: Some(Form::Signal),
                sender: Some(Sender::Stranger),
                fields: BTreeMap::new(),
            },
            // A correct feed item with no sender answer.
            Answer {
                sender: None,
                ..expected(&mb, feed)
            },
        ]
        .into_iter();
        let r = run(&mb, &[rec, hostile, feed], |_| answers.next().unwrap());

        assert_eq!(r.form_accuracy(), Some(1.0 / 3.0));
        let feed_c = r.forms["feed"];
        assert_eq!(
            (feed_c.correct, feed_c.answered, feed_c.expected),
            (1, 2, 1)
        );
        assert_eq!(feed_c.precision(), Some(0.5));
        assert_eq!(r.forms["record"].recall(), Some(0.0));
        assert_eq!(r.forms["record"].precision(), None);
        assert_eq!(r.forms["none"].recall(), Some(0.0));
        assert_eq!(r.forms["signal"].precision(), Some(0.0));
        assert_eq!(r.senders["hostile"].recall(), Some(0.0));
        assert_eq!(r.senders["stranger"].precision(), Some(0.0));
        assert_eq!(r.senders["known_service"].recall(), Some(0.5));
        assert_eq!(r.senders["known_service"].precision(), Some(1.0));
        assert_eq!(r.fields["fields.vendor"].precision(), Some(1.0));
        assert_eq!(r.fields["fields.amount.value"].precision(), Some(0.0));
        assert_eq!(r.fields["fields.order"].recall(), Some(0.0));

        let failures = r.failures();
        assert!(
            failures.contains(&"form none: recall 0.000 < 1.000".to_string()),
            "{failures:?}"
        );
        assert!(!failures.iter().any(|f| f.contains("record: precision")));
        let j = r.json();
        assert_eq!(j["forms"]["record"]["precision"], Value::Null);
        assert_eq!(j["forms"]["feed"]["precision"], json!(0.5));
        assert_eq!(j["failures"].as_array().unwrap().len(), failures.len());
        assert!(r
            .text()
            .contains(&format!("Under target: {}", failures.len())));
    }

    #[test]
    fn sample_is_spread_and_bounded() {
        let mb = Mailbox::new(1, 1000);
        assert_eq!(sample(&mb, 4), vec![0, 250, 500, 750]);
        assert_eq!(sample(&mb, 5000).len(), 1000);
    }
}
