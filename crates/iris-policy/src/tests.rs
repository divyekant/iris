use super::*;

/// The example of spec 2 section 6.1, read from the spec, so that the spec
/// and the code cannot drift apart.
fn spec_example() -> String {
    let spec = include_str!("../../../docs/spec/02-changeset-policy-ledger.md");
    let start = spec.find("### 6.1").expect("section 6.1");
    let block = &spec[start..];
    let open = block.find("```toml\n").expect("toml block") + "```toml\n".len();
    let close = block[open..].find("```").expect("end of block");
    block[open..open + close].to_string()
}

fn err(text: &str) -> Error {
    parse(text).expect_err("the policy must not be valid")
}

const HEAD: &str = "version = 0\nmode = \"apply\"\n";

#[test]
fn the_spec_example_is_valid() {
    let p = parse(&spec_example()).unwrap();
    assert_eq!(p.mode, Mode::Apply);
    assert_eq!(p.rules.len(), 4);
    let r = &p.rules[0];
    assert_eq!((r.id, r.effect, r.by), (1, Effect::Allow, By::Person));
    assert_eq!(r.verbs, vec![Verb::Archive, Verb::Read]);
    assert_eq!(r.when.form, Some(Form::Feed));
    assert_eq!(r.when.sender_kind, Some(SenderKind::Service));
    assert_eq!(r.when.older_than_days, Some(3));
    assert_eq!(r.added, "2026-11-10");
    assert_eq!(
        p.rules[1].when.sender.as_deref(),
        Some("deals@acme.example")
    );
    assert_eq!(p.rules[3].when.sender.as_deref(), Some("@bank.example"));
    // Each rule knows its line, so that the brief can point to it.
    let text = spec_example();
    for r in &p.rules {
        assert_eq!(
            text.lines().nth(r.line - 1).unwrap().trim(),
            "[[rule]]",
            "rule {}",
            r.id
        );
    }
}

#[test]
fn errors_name_the_line() {
    let rule = |body: &str| format!("{HEAD}\n[[rule]]\n{body}\n");
    let base = "id = 1\nname = \"N\"\neffect = \"allow\"\nverbs = [\"archive\"]\nwhen = { form = \"feed\" }\nby = \"person\"\nadded = 2026-11-10";
    assert!(parse(&rule(base)).is_ok());
    let cases: &[(&str, &str, &str)] = &[
        (
            "when = { form = \"feed\" }",
            "when = { from = \"feed\" }",
            "unknown field",
        ),
        (
            "when = { form = \"feed\" }",
            "when = {}",
            "when has no condition",
        ),
        (
            "when = { form = \"feed\" }",
            "when = { form = \"news\" }",
            "form \"news\"",
        ),
        (
            "when = { form = \"feed\" }",
            "when = { sender = \"acme\" }",
            "not an address",
        ),
        (
            "when = { form = \"feed\" }",
            "when = { sender = \"a@\" }",
            "not an address",
        ),
        (
            "when = { form = \"feed\" }",
            "when = { older_than_days = -1 }",
            "older_than_days",
        ),
        (
            "verbs = [\"archive\"]",
            "verbs = [\"send\"]",
            "verb \"send\"",
        ),
        ("verbs = [\"archive\"]", "verbs = []", "verbs is empty"),
        ("verbs = [\"archive\"]\n", "", "needs verbs"),
        (
            "effect = \"allow\"",
            "effect = \"permit\"",
            "effect \"permit\"",
        ),
        ("id = 1", "id = 0", "positive"),
        ("name = \"N\"", "name = \"  \"", "name is empty"),
        ("name = \"N\"", "name = \"a\\nb\"", "one line"),
        ("name = \"N\"", "name = \"\\na\"", "one line"),
        ("name = \"N\"", "name = \"a\\r\\n\"", "one line"),
        ("name = \"N\"", "name = \"\\t\"", "one line"),
        ("by = \"person\"", "by = \"agent\"", "by \"agent\""),
        (
            "added = 2026-11-10",
            "added = 2026-11-10T10:00:00Z",
            "must be a date",
        ),
        ("added = 2026-11-10", "added = \"2026-11-10\"", "datetime"),
    ];
    for (from, to, want) in cases {
        let text = rule(&base.replacen(from, to, 1));
        let e = err(&text);
        assert!(e.message.contains(want), "{to}: {e}");
        // A TOML error names the line of the field; a check names the rule.
        let field = text
            .lines()
            .position(|l| !to.is_empty() && l == *to)
            .map(|i| i + 1);
        assert!(e.line == 4 || Some(e.line) == field, "{to}: {e}");
    }
}

#[test]
fn attention_rules_have_no_verbs_author_or_job() {
    let rule = |extra: &str| {
        format!("{HEAD}[[rule]]\nid = 1\nname = \"N\"\neffect = \"hide\"\n{extra}\nby = \"person\"\nadded = 2026-11-10\n")
    };
    assert!(parse(&rule("when = { form = \"feed\" }")).is_ok());
    assert!(
        err(&rule("verbs = [\"archive\"]\nwhen = { form = \"feed\" }"))
            .message
            .contains("has no verbs")
    );
    assert!(err(&rule("when = { author = \"agent\" }"))
        .message
        .contains("author or job"));
    assert!(err(&rule("when = { form = \"feed\" }\njob = \"j-1\""))
        .message
        .contains("author or job"));
}

#[test]
fn top_level_errors() {
    assert_eq!(err("version = 1\nmode = \"apply\"\n").line, 1);
    let e = err("version = 0\nmode = \"auto\"\n");
    assert_eq!(e.line, 2);
    assert!(e.message.contains("\"auto\""));
    // A typo in [[rule]] must not drop each rule without a word.
    assert!(err(&format!("{HEAD}[[rules]]\nid = 1\n"))
        .message
        .contains("unknown field"));
    assert!(err("mode = \"apply\"\n").message.contains("version"));
    // A TOML syntax error names its line.
    assert_eq!(err(&format!("{HEAD}\n\nid = = 1\n")).line, 5);
}

#[test]
fn a_reused_id_is_an_error() {
    let text = spec_example().replacen("id = 2", "id = 1", 1);
    let e = err(&text);
    assert!(e.message.contains("id 1"), "{e}");
    assert_eq!(text.lines().nth(e.line - 1).unwrap(), "[[rule]]");
}

#[test]
fn limits_of_size() {
    let big = format!("{HEAD}#{}\n", "x".repeat(MAX_BYTES));
    assert!(err(&big).message.contains("larger"));
    let mut many = HEAD.to_string();
    for id in 1..=MAX_RULES + 1 {
        many += &format!("[[rule]]\nid = {id}\nname = \"n\"\neffect = \"bundle\"\nwhen = {{ form = \"feed\" }}\nby = \"person\"\nadded = 2026-11-10\n");
    }
    assert!(many.len() < MAX_BYTES);
    assert!(err(&many).message.contains("more than"));
}

#[test]
fn load_reads_the_file_and_its_hash() {
    let dir = std::env::temp_dir().join(format!("iris-policy-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(FILE);
    let _ = std::fs::remove_file(&path);
    assert_eq!(load(&dir), Ok(None));

    std::fs::write(&path, spec_example()).unwrap();
    let got = load(&dir).unwrap().unwrap();
    assert_eq!(got.text, spec_example());
    assert_eq!(got.policy.rules.len(), 4);
    assert_eq!(got.sha256, sha256_hex(&spec_example()));
    assert_eq!(
        sha256_hex(""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
    );

    std::fs::write(&path, [b'#', 0xff, b'\n']).unwrap();
    assert!(load(&dir).unwrap_err().message.contains("UTF-8"));
    std::fs::write(&path, vec![b'#'; MAX_BYTES + 10]).unwrap();
    assert!(load(&dir).unwrap_err().message.contains("larger"));
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn the_first_start_policy_observes_with_no_rules() {
    let p = Policy::default();
    assert_eq!(p.mode, Mode::Observe);
    assert!(p.rules.is_empty());
}
