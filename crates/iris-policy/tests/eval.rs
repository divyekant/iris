//! Tests of spec 2 section 6.4 (evaluation) and section 6.5 (hard limits).

use iris_policy::*;

const DAY: i64 = 86_400;
const NOW: i64 = 1_800_000_000;

fn policy(rules: &str) -> Policy {
    parse(&format!("version = 0\nmode = \"apply\"\n{rules}")).unwrap()
}

fn rule(id: u32, effect: &str, verbs: &str, when: &str) -> String {
    let verbs = if verbs.is_empty() {
        String::new()
    } else {
        format!("verbs = [{verbs}]\n")
    };
    format!("[[rule]]\nid = {id}\nname = \"r{id}\"\neffect = \"{effect}\"\n{verbs}when = {{ {when} }}\nby = \"person\"\nadded = 2026-11-10\n")
}

const FEED: Item = Item {
    form: Form::Feed,
    record_type: None,
    commitment_kind: None,
};
const RECEIPT: Item = Item {
    form: Form::Record,
    record_type: Some(RecordType::Receipt),
    commitment_kind: None,
};

fn msg<'a>(sender: &'a str, kind: SenderKind, items: &'a [Item]) -> Message<'a> {
    Message {
        sender,
        sender_kind: kind,
        hostile: false,
        labels: &[],
        date: NOW - 10 * DAY,
        items,
    }
}

fn action(verb: Verb, author: Author) -> Action<'static> {
    Action {
        verb,
        author,
        job: None,
        bulk: false,
        warning: false,
        in_thread: false,
        now: NOW,
    }
}

#[test]
fn default_is_review() {
    let d = policy("").decide(
        &action(Verb::Archive, Author::Agent),
        [msg("a@x.example", SenderKind::Person, &[FEED])],
    );
    assert_eq!((d.outcome, d.rule), (Outcome::Review, None));
    assert_eq!(d.reason, "No rule allows archive for these messages.");
}

#[test]
fn deny_beats_review_beats_allow() {
    let a = rule(3, "allow", "\"archive\"", "form = \"feed\"");
    let r = rule(2, "review", "\"archive\"", "sender_kind = \"service\"");
    let d = rule(1, "deny", "\"archive\"", "sender = \"@acme.example\"");
    let m = [msg("news@acme.example", SenderKind::Service, &[FEED])];
    let act = action(Verb::Archive, Author::Agent);

    let got = policy(&a).decide(&act, m);
    assert_eq!((got.outcome, got.rule), (Outcome::Allow, Some(3)));
    assert_eq!(got.reason, "Rule 3 (r3) allows archive.");
    let got = policy(&(a.clone() + &r)).decide(&act, m);
    assert_eq!((got.outcome, got.rule), (Outcome::Review, Some(2)));
    assert_eq!(got.reason, "Rule 2 (r2) sends archive to review.");
    let got = policy(&(a + &r + &d)).decide(&act, m);
    assert_eq!((got.outcome, got.rule), (Outcome::Deny, Some(1)));
    assert_eq!(got.reason, "Rule 1 (r1) denies archive.");
}

#[test]
fn a_rule_applies_only_to_its_verbs() {
    let p = policy(&rule(1, "allow", "\"archive\"", "form = \"feed\""));
    let m = [msg("a@x.example", SenderKind::Service, &[FEED])];
    assert_eq!(
        p.decide(&action(Verb::Trash, Author::Agent), m).outcome,
        Outcome::Review
    );
}

#[test]
fn one_deny_message_in_a_thread_of_ten_denies() {
    // Spec 2 section 10: an allow rule matches each message, and one message
    // is from the sender of a deny rule.
    let p = policy(
        &(rule(1, "allow", "\"archive\"", "older_than_days = 3")
            + &rule(2, "deny", "\"archive\"", "sender = \"boss@corp.example\"")),
    );
    let mut thread: Vec<Message> = (0..9)
        .map(|_| msg("a@x.example", SenderKind::Person, &[]))
        .collect();
    thread.push(msg("Boss@Corp.Example", SenderKind::Person, &[]));
    let d = p.decide(
        &action(Verb::Archive, Author::Agent),
        thread.iter().copied(),
    );
    assert_eq!((d.outcome, d.rule), (Outcome::Deny, Some(2)));
    // Without the deny message, the allow rule matches each message.
    let d = p.decide(
        &action(Verb::Archive, Author::Agent),
        thread[..9].iter().copied(),
    );
    assert_eq!((d.outcome, d.rule), (Outcome::Allow, Some(1)));
}

#[test]
fn allow_needs_each_message_and_each_item() {
    let p = policy(&rule(1, "allow", "\"archive\"", "form = \"feed\""));
    let act = action(Verb::Archive, Author::Agent);
    let s = SenderKind::Service;
    assert_eq!(
        p.decide(&act, [msg("a@x.example", s, &[FEED, FEED])])
            .outcome,
        Outcome::Allow
    );
    // A feed item and a receipt: not each item is a feed.
    assert_eq!(
        p.decide(&act, [msg("a@x.example", s, &[FEED, RECEIPT])])
            .outcome,
        Outcome::Review
    );
    // A message with no item does not match an item condition.
    assert_eq!(
        p.decide(&act, [msg("a@x.example", s, &[])]).outcome,
        Outcome::Review
    );
    // One message of two is not a feed.
    assert_eq!(
        p.decide(
            &act,
            [
                msg("a@x.example", s, &[FEED]),
                msg("b@x.example", s, &[RECEIPT])
            ]
        )
        .outcome,
        Outcome::Review
    );
    // No message: an allow rule never matches.
    assert_eq!(p.decide(&act, []).outcome, Outcome::Review);
}

#[test]
fn review_and_deny_need_one_item() {
    let act = action(Verb::Archive, Author::Agent);
    let m = [msg("a@x.example", SenderKind::Service, &[FEED, RECEIPT])];
    for (effect, want) in [("review", Outcome::Review), ("deny", Outcome::Deny)] {
        let p = policy(
            &(rule(1, "allow", "\"archive\"", "sender_kind = \"service\"")
                + &rule(2, effect, "\"archive\"", "record_type = \"receipt\"")),
        );
        assert_eq!(p.decide(&act, m).outcome, want, "{effect}");
    }
}

#[test]
fn a_hostile_mark_matches_only_hostile() {
    let act = action(Verb::Archive, Author::Agent);
    let mut m = msg("a@x.example", SenderKind::Person, &[]);
    m.hostile = true;
    let p = policy(&rule(1, "allow", "\"archive\"", "sender_kind = \"person\""));
    assert_eq!(p.decide(&act, [m]).outcome, Outcome::Review);
    let p = policy(&rule(
        1,
        "allow",
        "\"archive\"",
        "sender_kind = \"hostile\"",
    ));
    assert_eq!(p.decide(&act, [m]).outcome, Outcome::Allow);
}

#[test]
fn hard_limit_hostile_mail() {
    let p = policy(&rule(
        1,
        "allow",
        "\"archive\", \"read\", \"label\"",
        "sender_kind = \"hostile\"",
    ));
    let m = [msg("x@evil.example", SenderKind::Hostile, &[])];
    let mut marked = msg("y@x.example", SenderKind::Person, &[]);
    marked.hostile = true;
    for author in [Author::Agent, Author::Triage, Author::Local] {
        let d = p.decide(&action(Verb::Read, author), m);
        assert_eq!((d.outcome, d.rule), (Outcome::Deny, None), "{author:?}");
        assert!(d.reason.contains("hostile"), "{}", d.reason);
        // A marked message among others counts too.
        let d = p.decide(
            &action(Verb::Read, author),
            [msg("a@x.example", SenderKind::Person, &[]), marked],
        );
        assert_eq!(d.outcome, Outcome::Deny);
        assert_eq!(
            p.decide(&action(Verb::Archive, author), m).outcome,
            Outcome::Allow
        );
        assert_eq!(
            p.decide(&action(Verb::Label, author), m).outcome,
            Outcome::Allow
        );
    }
    // The hard limit does not apply to the person.
    assert_eq!(
        p.decide(&action(Verb::Read, Author::Person), m).outcome,
        Outcome::Allow
    );
}

#[test]
fn hard_limit_bulk() {
    let p = policy(&rule(1, "allow", "\"archive\"", "form = \"feed\""));
    let m = [msg("a@x.example", SenderKind::Service, &[FEED])];
    let mut act = action(Verb::Archive, Author::Person);
    act.bulk = true;
    let d = p.decide(&act, m);
    assert_eq!((d.outcome, d.rule), (Outcome::Review, None));
    assert!(d.reason.contains("bulk"));
    // A rule can make a hard limit stricter.
    let p = policy(&rule(1, "deny", "\"archive\"", "form = \"feed\""));
    let d = p.decide(&act, m);
    assert_eq!((d.outcome, d.rule), (Outcome::Deny, Some(1)));
}

#[test]
fn author_job_label_and_age() {
    let m = |date: i64, labels: &'static [String]| Message {
        date,
        labels,
        ..msg("a@x.example", SenderKind::Service, &[])
    };
    let agent = action(Verb::Archive, Author::Agent);

    let p = policy(&rule(1, "allow", "\"archive\"", "author = \"triage\""));
    assert_eq!(p.decide(&agent, [m(0, &[])]).outcome, Outcome::Review);
    assert_eq!(
        p.decide(&action(Verb::Archive, Author::Triage), [m(0, &[])])
            .outcome,
        Outcome::Allow
    );

    let p =
        policy(&(rule(1, "allow", "\"archive\"", "sender_kind = \"service\"") + "job = \"j-1\"\n"));
    assert_eq!(p.decide(&agent, [m(0, &[])]).outcome, Outcome::Review);
    assert_eq!(
        p.decide(
            &Action {
                job: Some("j-2"),
                ..agent
            },
            [m(0, &[])]
        )
        .outcome,
        Outcome::Review
    );
    assert_eq!(
        p.decide(
            &Action {
                job: Some("j-1"),
                ..agent
            },
            [m(0, &[])]
        )
        .outcome,
        Outcome::Allow
    );

    let labels: &'static [String] = Box::leak(vec!["Newsletters".to_string()].into_boxed_slice());
    let p = policy(&rule(1, "allow", "\"archive\"", "label = \"newsletters\""));
    assert_eq!(p.decide(&agent, [m(0, labels)]).outcome, Outcome::Allow);
    assert_eq!(p.decide(&agent, [m(0, &[])]).outcome, Outcome::Review);

    // older_than_days = 3 needs more than 3 days.
    let p = policy(&rule(1, "allow", "\"archive\"", "older_than_days = 3"));
    assert_eq!(
        p.decide(&agent, [m(NOW - 3 * DAY, &[])]).outcome,
        Outcome::Review
    );
    assert_eq!(
        p.decide(&agent, [m(NOW - 3 * DAY - 1, &[])]).outcome,
        Outcome::Allow
    );
}

#[test]
fn sender_domain_matches_only_that_domain() {
    let p = policy(&rule(
        1,
        "allow",
        "\"archive\"",
        "sender = \"@bank.example\"",
    ));
    let act = action(Verb::Archive, Author::Agent);
    let k = SenderKind::Service;
    assert_eq!(
        p.decide(&act, [msg("Alerts@BANK.example", k, &[])]).outcome,
        Outcome::Allow
    );
    for other in [
        "a@mybank.example",
        "a@bank.example.evil",
        "a@sub.bank.example",
        "",
        "@bank.example",
    ] {
        assert_eq!(
            p.decide(&act, [msg(other, k, &[])]).outcome,
            Outcome::Review,
            "{other}"
        );
    }
}

#[test]
fn attention_takes_the_lowest_id() {
    let p = policy(
        &(rule(5, "hide", "", "sender = \"deals@acme.example\"")
            + &rule(3, "bundle", "", "form = \"feed\"")),
    );
    let m = msg("deals@acme.example", SenderKind::Service, &[FEED]);
    assert_eq!(p.attention(&FEED, &m, NOW), Attention::Bundled { rule: 3 });
    assert_eq!(
        p.attention(&RECEIPT, &m, NOW),
        Attention::Hidden { rule: 5 }
    );
    let other = msg("b@x.example", SenderKind::Person, &[RECEIPT]);
    assert_eq!(p.attention(&RECEIPT, &other, NOW), Attention::Shown);
    // Action rules never change attention.
    let p = policy(&rule(1, "deny", "\"archive\"", "form = \"feed\""));
    assert_eq!(p.attention(&FEED, &m, NOW), Attention::Shown);
}

// Property tests. A small generator with a fixed seed keeps each run the same.

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[(self.next() % xs.len() as u64) as usize]
    }
}

const SENDERS: [&str; 4] = ["a@x.example", "b@x.example", "c@y.example", "d@z.example"];
const KINDS: [SenderKind; 5] = [
    SenderKind::SelfOwner,
    SenderKind::Person,
    SenderKind::Service,
    SenderKind::Stranger,
    SenderKind::Hostile,
];
const ITEMS: [Item; 3] = [
    FEED,
    RECEIPT,
    Item {
        form: Form::Commitment,
        record_type: None,
        commitment_kind: Some(CommitmentKind::Reply),
    },
];

fn random_rules(rng: &mut Rng, n: usize) -> Vec<String> {
    let conds = [
        "form = \"feed\"",
        "form = \"record\"",
        "record_type = \"receipt\"",
        "commitment_kind = \"reply\"",
        "sender = \"@x.example\"",
        "sender = \"c@y.example\"",
        "sender_kind = \"service\"",
        "sender_kind = \"hostile\"",
        "author = \"agent\"",
        "older_than_days = 5",
    ];
    let effects = ["allow", "allow", "review", "deny"];
    let verbs = [
        "\"archive\"",
        "\"read\"",
        "\"archive\", \"read\"",
        "\"trash\"",
    ];
    (1..=n as u32)
        .map(|id| {
            let mut when = rng.pick(&conds).to_string();
            if rng.next().is_multiple_of(2) {
                let c = rng.pick(&conds);
                if !when.starts_with(&c[..c.find(' ').unwrap()]) {
                    when = format!("{when}, {c}");
                }
            }
            rule(id, rng.pick(&effects), rng.pick(&verbs), &when)
        })
        .collect()
}

struct Case {
    msgs: Vec<(usize, usize, bool, i64, Vec<Item>)>,
    act: Action<'static>,
}

fn random_case(rng: &mut Rng) -> Case {
    let n = (rng.next() % 4) as usize;
    let msgs = (0..n)
        .map(|_| {
            let items = (0..rng.next() % 3).map(|_| *rng.pick(&ITEMS)).collect();
            (
                (rng.next() % 4) as usize,
                (rng.next() % 5) as usize,
                rng.next().is_multiple_of(6),
                NOW - (rng.next() % 10) as i64 * DAY,
                items,
            )
        })
        .collect();
    let act = Action {
        verb: *rng.pick(&[Verb::Archive, Verb::Read, Verb::Trash]),
        author: *rng.pick(Author::ALL),
        job: None,
        bulk: rng.next().is_multiple_of(5),
        warning: false,
        in_thread: false,
        now: NOW,
    };
    Case { msgs, act }
}

fn run(p: &Policy, c: &Case) -> Decision {
    p.decide(
        &c.act,
        c.msgs.iter().map(|(s, k, h, d, items)| Message {
            sender: SENDERS[*s],
            sender_kind: KINDS[*k],
            hostile: *h,
            labels: &[],
            date: *d,
            items,
        }),
    )
}

#[test]
fn property_invariants_of_evaluation() {
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for _ in 0..3000 {
        let n = (rng.next() % 6) as usize;
        let rules = random_rules(&mut rng, n);
        let c = random_case(&mut rng);
        let full = policy(&rules.concat());
        let got = run(&full, &c);

        // No rules: never allow. A hard limit is a floor that no rule lowers.
        let none = run(&policy(""), &c);
        assert_ne!(none.outcome, Outcome::Allow);
        let hard = if none.reason.starts_with("No rule") {
            Outcome::Allow
        } else {
            none.outcome
        };
        assert!(
            got.outcome >= hard,
            "{got:?} is below the hard limit {none:?}"
        );

        // The order of the rules in the file does not change the result.
        let mut rev = rules.clone();
        rev.reverse();
        assert_eq!(run(&policy(&rev.concat()), &c), got);

        // Fail closed: a rule that is not allow never makes a result less
        // strict, and an allow rule never makes it stricter.
        for (i, r) in rules.iter().enumerate() {
            let mut fewer = rules.clone();
            fewer.remove(i);
            let without = run(&policy(&fewer.concat()), &c);
            if r.contains("effect = \"allow\"") {
                assert!(
                    without.outcome >= got.outcome,
                    "removing {r} gave {without:?}, with it {got:?}"
                );
            } else {
                assert!(
                    without.outcome <= got.outcome,
                    "removing {r} gave {without:?}, with it {got:?}"
                );
            }
        }

        // The reason names the rule that decided.
        match got.rule {
            Some(id) => assert!(got.reason.starts_with(&format!("Rule {id} "))),
            None => assert!(!got.reason.is_empty()),
        }
    }
}

#[test]
fn property_parse_never_panics() {
    // Mutations of a valid file: each one parses or gives an error with a
    // line in the file.
    let base = rule(
        1,
        "allow",
        "\"archive\"",
        "form = \"feed\", sender = \"@x.example\"",
    ) + &rule(2, "hide", "", "sender_kind = \"stranger\"");
    let text = format!("version = 0\nmode = \"apply\"\n{base}");
    let alphabet = b"[]{}=\"#\n ,.@-_0123456789abcdefghijklmnopqrstuvwxyz\\\x00\xc3\xa9";
    let mut rng = Rng(42);
    for _ in 0..5000 {
        let mut bytes = text.as_bytes().to_vec();
        for _ in 0..1 + rng.next() % 4 {
            let at = (rng.next() as usize) % (bytes.len() + 1);
            match rng.next() % 3 {
                0 if at < bytes.len() => {
                    bytes.remove(at);
                }
                1 if at < bytes.len() => bytes[at] = *rng.pick(alphabet),
                _ => bytes.insert(at, *rng.pick(alphabet)),
            }
        }
        let Ok(s) = String::from_utf8(bytes) else {
            continue;
        };
        if let Err(e) = parse(&s) {
            assert!(e.line <= s.lines().count() + 1, "{e} in {s:?}");
            assert!(!e.message.is_empty());
        }
    }
}

/// Decision D3: `recipients = "thread"` matches only a draft whose
/// recipients are in its thread. Spec 2 section 6.5: a draft with a warning
/// flag goes to review, even if a rule allows it.
#[test]
fn a_draft_rule_reads_the_recipients_and_the_warning_flags() {
    let p = policy(&rule(
        1,
        "allow",
        "\"draft\"",
        "recipients = \"thread\", author = \"agent\"",
    ));
    let m = || [msg("a@x.example", SenderKind::Person, &[])];
    let draft = |in_thread, warning| Action {
        in_thread,
        warning,
        ..action(Verb::Draft, Author::Agent)
    };
    let d = p.decide(&draft(true, false), m());
    assert_eq!((d.outcome, d.rule), (Outcome::Allow, Some(1)));
    assert_eq!(p.decide(&draft(false, false), m()).outcome, Outcome::Review);
    let d = p.decide(&draft(true, true), m());
    assert_eq!((d.outcome, d.rule), (Outcome::Review, None));
    assert!(d.reason.contains("warning flag"), "{}", d.reason);
    // A new mail has no message: an allow rule never matches it.
    assert_eq!(p.decide(&draft(true, false), []).outcome, Outcome::Review);
    // The rule names only `draft`.
    assert_eq!(
        p.decide(
            &Action {
                in_thread: true,
                ..action(Verb::Archive, Author::Agent)
            },
            m()
        )
        .outcome,
        Outcome::Review
    );
    // A rule with no `recipients` allows a draft to each address.
    let p = policy(&rule(1, "allow", "\"draft\"", "sender_kind = \"person\""));
    assert_eq!(p.decide(&draft(false, false), m()).outcome, Outcome::Allow);
    // A hostile thread: only a person drafts in it.
    let mut h = msg("a@x.example", SenderKind::Person, &[]);
    h.hostile = true;
    let p = policy(&rule(1, "allow", "\"draft\"", "author = \"agent\""));
    assert_eq!(p.decide(&draft(true, false), [h]).outcome, Outcome::Deny);
}
