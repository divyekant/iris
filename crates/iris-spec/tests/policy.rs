//! The policy schema agrees with the policy parser of the engine
//! (`iris_policy::parse`). A policy file is TOML, so the test reads each file
//! as TOML, turns it into JSON (a TOML date becomes a string), and checks it
//! against `policy.schema.json`.

use std::fs;
use std::path::Path;

use iris_spec::{schema_dir, validator};
use jsonschema::Validator;
use serde_json::Value;

/// The JSON form of a TOML value. A date becomes its text.
fn json(v: toml::Value) -> Value {
    match v {
        toml::Value::String(s) => Value::String(s),
        toml::Value::Integer(i) => Value::from(i),
        toml::Value::Float(f) => Value::from(f),
        toml::Value::Boolean(b) => Value::Bool(b),
        toml::Value::Datetime(d) => Value::String(d.to_string()),
        toml::Value::Array(a) => Value::Array(a.into_iter().map(json).collect()),
        toml::Value::Table(t) => Value::Object(t.into_iter().map(|(k, v)| (k, json(v))).collect()),
    }
}

/// The schema verdict on a TOML text. A text that is not TOML is not valid.
fn schema_accepts(schema: &Validator, text: &str) -> bool {
    match toml::from_str::<toml::Table>(text) {
        Ok(t) => schema.is_valid(&json(toml::Value::Table(t))),
        Err(_) => false,
    }
}

fn samples(dir: &Path) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            (name, fs::read_to_string(&p).unwrap())
        })
        .collect();
    out.sort();
    out
}

#[test]
fn policy_samples_pass_and_invalid_samples_fail_in_the_schema_and_the_parser() {
    let schema = validator("policy");
    let valid = samples(&schema_dir().join("examples"));
    assert!(valid.len() >= 2, "expected policy samples");
    for (name, text) in valid {
        assert!(schema_accepts(&schema, &text), "{name}: schema must accept");
        assert!(
            iris_policy::parse(&text).is_ok(),
            "{name}: parser must accept"
        );
    }
    let invalid = samples(&schema_dir().join("examples/invalid"));
    assert!(!invalid.is_empty(), "expected invalid policy samples");
    for (name, text) in invalid {
        assert!(
            !schema_accepts(&schema, &text),
            "{name}: schema must refuse"
        );
        assert!(
            iris_policy::parse(&text).is_err(),
            "{name}: parser must refuse"
        );
    }
}

/// xorshift64: a fixed sequence, so a failure repeats.
struct Rng(u64);

impl Rng {
    fn pick<'a>(&mut self, from: &[&'a str]) -> &'a str {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        from[(self.0 % from.len() as u64) as usize]
    }
}

/// A TOML string, with escapes. JSON escapes are also TOML escapes.
fn s(text: &str) -> String {
    serde_json::to_string(text).unwrap()
}

/// One field of a rule: its key, values that the parser accepts, and values
/// that it refuses. An empty value leaves the field out.
type Field = (
    &'static str,
    &'static [&'static str],
    &'static [&'static str],
);

const ID: Field = ("id", &["N"], &["0", "-1", "4294967296", "\"1\"", ""]);
const NAME: Field = (
    "name",
    &[
        "\"Archive newsletters\"",
        "\"  Keep a space  \"",
        "\"café\"",
    ],
    &[
        "\"\"",
        "\"   \"",
        "\"two\\nlines\"",
        "\"a\\ttab\"",
        "\"next\\u0085line\"",
        "3",
        "",
    ],
);
const BY: Field = ("by", &["\"person\"", "\"proposal\""], &["\"bot\"", ""]);
const ADDED: Field = (
    "added",
    &["2026-11-10"],
    &[
        "2026-11-10T10:00:00Z",
        "2026-11-10T10:00:00",
        "10:00:00",
        "",
    ],
);
const JOB: Field = ("job", &["", "\"j-1\""], &["\" \"", "1"]);
const ACTION: Field = (
    "verbs",
    &["[\"archive\"]", "[\"archive\", \"read\", \"close\"]"],
    &["[]", "[\"untrash\"]", "[\"send\"]", "\"archive\"", ""],
);
/// Conditions of `when`. The bad values hold the cases of the parser.
const WHEN: [Field; 8] = [
    ("form", &["\"feed\"", "\"record\""], &["\"mail\"", "1"]),
    (
        "sender",
        &[
            "\"a@shop.example\"",
            "\"@Shop.Example\"",
            "\"  @shop.example \"",
        ],
        &[
            "\"shop.example\"",
            "\"a@b@c\"",
            "\"a b@c\"",
            "\"@\"",
            "\"a@\"",
            "\"a\\u00a0b@c\"",
            "\"\"",
        ],
    ),
    ("sender_kind", &["\"service\"", "\"self\""], &["\"bot\""]),
    ("label", &["\"Finance\""], &["\" \"", "\"\""]),
    ("record_type", &["\"receipt\""], &["\"invoice\""]),
    ("commitment_kind", &["\"payment\""], &["\"chore\""]),
    ("author", &["\"agent\""], &["\"robot\""]),
    (
        "older_than_days",
        &["3", "0", "4294967295"],
        &["-1", "4294967296", "\"3\""],
    ),
];

/// Makes a policy text: valid rules, where each rule has at most one defect.
/// So each defect is tested alone, and not hidden by another one. Ids differ,
/// because a schema cannot compare two rules (the parser refuses a duplicate
/// id).
fn policy(r: &mut Rng) -> String {
    let mut t = String::new();
    let top = r.pick(&["ok", "ok", "ok", "ok", "ok", "version", "mode", "extra"]);
    t += if top == "version" {
        "version = 1\n"
    } else {
        "version = 0\n"
    };
    t += if top == "mode" {
        "mode = \"auto\"\n"
    } else {
        r.pick(&["mode = \"apply\"\n", "mode = \"observe\"\n"])
    };
    if top == "extra" {
        t += "strict = true\n";
    }
    let rules = r.pick(&["0", "1", "1", "2"]).parse::<u64>().unwrap();
    for n in 0..rules {
        let effect = r.pick(&["allow", "review", "deny", "hide", "bundle"]);
        let action = matches!(effect, "allow" | "review" | "deny");
        // The field with the defect, or "none".
        let bad = r.pick(&[
            "none",
            "none",
            "none",
            "id",
            "name",
            "by",
            "added",
            "job",
            "effect",
            "verbs",
            "when",
            "when",
            "when",
            "no-when",
            "empty-when",
            "unknown-when",
            "item-verbs",
            "item-author",
        ]);
        let field = |r: &mut Rng, f: Field| {
            let v = r.pick(if bad == f.0 { f.2 } else { f.1 });
            let v = if v == "N" {
                (n + 1).to_string()
            } else {
                v.to_string()
            };
            if v.is_empty() {
                String::new()
            } else {
                format!("{} = {v}\n", f.0)
            }
        };
        t += "\n[[rule]]\n";
        t += &field(r, ID);
        t += &field(r, NAME);
        t += &if bad == "effect" {
            "effect = \"block\"\n".to_string()
        } else {
            format!("effect = {}\n", s(effect))
        };
        if action {
            t += &field(r, ACTION);
        } else if bad == "item-verbs" {
            t += "verbs = [\"archive\"]\n";
        }
        let mut when = Vec::new();
        let broken = r
            .pick(&["0", "1", "2", "3", "4", "5", "6", "7"])
            .parse::<usize>()
            .unwrap();
        for (i, f) in WHEN.iter().enumerate() {
            let in_when = i == broken || r.pick(&["in", "out", "out"]) == "in";
            if !in_when || (f.0 == "author" && !action) {
                continue;
            }
            let v = r.pick(if bad == "when" && i == broken {
                f.2
            } else {
                f.1
            });
            when.push(format!("{} = {v}", f.0));
        }
        if bad == "unknown-when" {
            when.push("subject = \"x\"".into());
        }
        if bad == "item-author" && !action {
            when.push("author = \"agent\"".into());
        }
        if bad == "empty-when" {
            when.clear();
        }
        if bad != "no-when" {
            t += &format!("when = {{ {} }}\n", when.join(", "));
        }
        t += &field(r, BY);
        t += &field(r, ADDED);
        if !action && bad == "item-verbs" {
            // job on an item rule is refused too.
            t += r.pick(&["", "job = \"j-1\"\n"]);
        } else if action || bad == "job" {
            t += &field(r, JOB);
        }
    }
    t
}

/// The property: for each generated policy text, the schema and the parser
/// give the same verdict.
#[test]
fn the_schema_and_the_parser_agree_on_generated_policies() {
    let schema = validator("policy");
    let mut r = Rng(0x9e37_79b9_7f4a_7c15);
    let (mut ok, mut refused) = (0, 0);
    for _ in 0..10_000 {
        let text = policy(&mut r);
        let parser = iris_policy::parse(&text);
        assert_eq!(
            schema_accepts(&schema, &text),
            parser.is_ok(),
            "the schema and the parser differ on:\n{text}\nparser: {parser:?}"
        );
        if parser.is_ok() {
            ok += 1;
        } else {
            refused += 1;
        }
    }
    // Both verdicts occur, so the property tests both sides.
    assert!(ok > 500 && refused > 500, "ok {ok}, refused {refused}");
}
