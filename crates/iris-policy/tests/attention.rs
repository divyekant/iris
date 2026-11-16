//! Tests of spec 2 sections 6.3 and 6.4: after each change of the active
//! policy, the `attention` field of each item agrees with the policy.

use iris_policy::*;
use iris_store::Store;
use std::path::PathBuf;

const DAY: i64 = 86_400;
const NOW: i64 = 1_800_000_000;

struct Dir(PathBuf);
impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn store(name: &str) -> (Dir, Store) {
    let dir = std::env::temp_dir().join(format!("iris-attn-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let store = Store::open(&dir).unwrap();
    (Dir(dir), store)
}

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
    fn coin(&mut self) -> bool {
        self.next().is_multiple_of(2)
    }
}

/// A sender as the header shows it, and its row in `sender`. "" is a
/// message with no header.
const SENDERS: [(&str, Option<&str>); 6] = [
    ("ann@friends.example", Some("person")),
    ("News@Shop.example", Some("service")),
    ("deals@shop.example", None),
    ("x@far.example", Some("hostile")),
    ("unknown@else.example", None),
    ("", None),
];
const LABELS: [&str; 3] = ["INBOX", "Promotions", "Work"];
const FORMS: [&str; 5] = ["commitment", "conversation", "record", "feed", "signal"];
const TYPES: [&str; 3] = ["receipt", "ticket", "other"];
const KINDS: [&str; 3] = ["task", "reply", "payment"];

/// One message of the generated mailbox, with what the policy sees.
struct Msg {
    sender: String,
    kind: SenderKind,
    hostile: bool,
    labels: Vec<String>,
    date: i64,
    items: Vec<(i64, Item)>,
}

fn mailbox(store: &Store, rng: &mut Rng) -> Vec<Msg> {
    let mut out = Vec::new();
    store
        .write(|tx| {
            tx.execute_batch(
                "INSERT INTO account(id, provider, address) VALUES (1, 'gmail', 'me@x.example');
                 INSERT INTO thread(account_id, provider_id) VALUES (1, 't');",
            )?;
            for (i, l) in LABELS.iter().enumerate() {
                tx.execute(
                    "INSERT INTO label(id, account_id, provider_id, name, kind) VALUES (?1, 1, ?2, ?2, 'user')",
                    rusqlite::params![i as i64 + 1, l],
                )?;
            }
            for (a, k) in SENDERS {
                if let Some(k) = k {
                    tx.execute("INSERT INTO sender(address, kind) VALUES (?1, ?2)", [a.to_lowercase(), k.to_string()])?;
                }
            }
            // Two rows that differ only in case: the most cautious kind wins.
            tx.execute("INSERT INTO sender(address, kind) VALUES ('NEWS@shop.example', 'stranger')", [])?;
            for m in 0..40 {
                let (addr, k) = *rng.pick(&SENDERS);
                let hostile = rng.next().is_multiple_of(5);
                let date = NOW - (rng.next() % 60) as i64 * DAY;
                let in_header = rng.coin();
                tx.execute(
                    "INSERT INTO message(account_id, thread_id, provider_id, internal_date, hostile)
                     VALUES (1, 1, ?1, ?2, ?3)",
                    rusqlite::params![
                        format!("m{m}"),
                        if in_header { None } else { Some(date * 1000) },
                        if hostile { Some("[\"phishing\"]") } else { None },
                    ],
                )?;
                let id = tx.last_insert_rowid();
                if !addr.is_empty() {
                    tx.execute(
                        "INSERT INTO header(message_id, date, from_addr) VALUES (?1, ?2, ?3)",
                        rusqlite::params![id, if in_header { Some(date * 1000) } else { None }, addr],
                    )?;
                }
                let mut labels = Vec::new();
                for (i, l) in LABELS.iter().enumerate() {
                    if rng.coin() {
                        tx.execute(
                            "INSERT INTO message_label(message_id, label_id) VALUES (?1, ?2)",
                            [id, i as i64 + 1],
                        )?;
                        labels.push(l.to_string());
                    }
                }
                let mut items = Vec::new();
                for _ in 0..1 + rng.next() % 3 {
                    let form = *rng.pick(&FORMS);
                    let (doc, rt, ck) = match form {
                        "record" => {
                            let t = *rng.pick(&TYPES);
                            (format!(r#"{{"type":"{t}"}}"#), RecordType::from_name(t), None)
                        }
                        "commitment" => {
                            let c = *rng.pick(&KINDS);
                            (format!(r#"{{"kind":"{c}"}}"#), None, CommitmentKind::from_name(c))
                        }
                        _ => ("{}".to_string(), None, None),
                    };
                    tx.execute(
                        "INSERT INTO item(form, account_id, status, doc, created_at, updated_at)
                         VALUES (?1, 1, 'new', ?2, 0, 0)",
                        [form, &doc],
                    )?;
                    let item = tx.last_insert_rowid();
                    tx.execute(
                        "INSERT INTO item_source(item_id, message_id, role) VALUES (?1, ?2, 'origin')",
                        [item, id],
                    )?;
                    let it = Item {
                        form: Form::from_name(form).unwrap(),
                        record_type: rt,
                        commitment_kind: ck,
                    };
                    items.push((item, it));
                }
                // With no header, a date only in the header is no date.
                let date = if addr.is_empty() && in_header { NOW } else { date };
                out.push(Msg {
                    sender: addr.to_lowercase(),
                    kind: match addr {
                        "News@Shop.example" => SenderKind::Stranger,
                        _ => k.and_then(SenderKind::from_name).unwrap_or(SenderKind::Stranger),
                    },
                    hostile,
                    labels,
                    date,
                    items,
                });
            }
            Ok(())
        })
        .unwrap();
    out
}

fn random_policy(rng: &mut Rng, ids: &mut u32) -> String {
    let mut s = "version = 0\nmode = \"observe\"\n".to_string();
    for _ in 0..rng.next() % 5 {
        *ids += 1;
        let mut when = Vec::new();
        if rng.coin() {
            when.push(format!("form = \"{}\"", rng.pick(&FORMS)));
        }
        if rng.next().is_multiple_of(3) {
            when.push(format!(
                "sender_kind = \"{}\"",
                rng.pick(&["person", "service", "stranger", "hostile"])
            ));
        }
        if rng.next().is_multiple_of(3) {
            when.push(format!(
                "sender = \"{}\"",
                rng.pick(&["@shop.example", "ann@friends.example"])
            ));
        }
        if rng.next().is_multiple_of(3) {
            when.push(format!("label = \"{}\"", rng.pick(&LABELS)));
        }
        if rng.next().is_multiple_of(3) {
            when.push(format!("older_than_days = {}", rng.next() % 50));
        }
        if rng.next().is_multiple_of(4) {
            when.push(format!("record_type = \"{}\"", rng.pick(&TYPES)));
        }
        if when.is_empty() {
            when.push(format!("form = \"{}\"", rng.pick(&FORMS)));
        }
        s += &format!(
            "[[rule]]\nid = {}\nname = \"Rule \\\"{}\\\"\"\neffect = \"{}\"\nwhen = {{ {} }}\nby = \"person\"\nadded = 2026-11-10\n",
            ids,
            ids,
            rng.pick(&["hide", "bundle"]),
            when.join(", ")
        );
    }
    s
}

/// The `attention` field of each item, by item ID.
fn stored(store: &Store) -> Vec<(i64, Option<String>)> {
    store
        .read(|c| {
            c.prepare("SELECT id, json_extract(doc, '$.attention') FROM item ORDER BY id")?
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()
                .map_err(Into::into)
        })
        .unwrap()
}

fn expected(policy: &Policy, mailbox: &[Msg]) -> Vec<(i64, Attention)> {
    let mut out = Vec::new();
    for m in mailbox {
        let all: Vec<Item> = m.items.iter().map(|(_, i)| *i).collect();
        let msg = Message {
            sender: &m.sender,
            sender_kind: m.kind,
            hostile: m.hostile,
            labels: &m.labels,
            date: m.date,
            items: &all,
        };
        for (id, it) in &m.items {
            out.push((*id, policy.attention(it, &msg, NOW)));
        }
    }
    out.sort_by_key(|(id, _)| *id);
    out
}

/// The `attention` JSON of the schema, as SQLite gives it back.
fn agrees(got: &Option<String>, want: Attention, policy: &Policy) -> bool {
    let (state, rule) = match want {
        Attention::Shown => return got.as_deref() == Some(r#"{"state":"shown"}"#),
        Attention::Hidden { rule } => ("hidden", rule),
        Attention::Bundled { rule } => ("bundled", rule),
    };
    let name = &policy.rules.iter().find(|r| r.id == rule).unwrap().name;
    let verb = if state == "hidden" {
        "Hidden"
    } else {
        "Bundled"
    };
    let reason = format!("{verb} by rule {rule}: {name}").replace('"', "\\\"");
    got.as_deref()
        == Some(&format!(
            r#"{{"state":"{state}","rule":{rule},"reason":"{reason}"}}"#
        ))
}

#[test]
fn each_policy_change_writes_the_attention_of_each_item() {
    // Invariant: after each confirmation and each change from the web
    // client, the attention of each item is the attention that the active
    // policy gives it.
    for seed in 1..=20u64 {
        let (_d, s) = store(&format!("p{seed}"));
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let mail = mailbox(&s, &mut rng);
        let mut ids = 0;
        for step in 0..4 {
            let text = format!("# step {step}\n{}", random_policy(&mut rng, &mut ids));
            std::fs::write(s.dir().join(FILE), &text).unwrap();
            let sha = match file_state(&s).unwrap() {
                FileState::Changed(l) => l.sha256,
                other => panic!("seed {seed} step {step}: {other:?}"),
            };
            let policy = match confirm(&s, &sha, NOW).unwrap() {
                Confirm::Done { policy, .. } => policy,
                other => panic!("seed {seed} step {step}: {other:?}"),
            };
            let got = stored(&s);
            let want = expected(&policy, &mail);
            assert_eq!(got.len(), want.len());
            for ((id, g), (_, w)) in got.iter().zip(want) {
                assert!(
                    agrees(g, w, &policy),
                    "seed {seed} step {step} item {id}: {g:?} != {w:?}\n{text}"
                );
            }
            // A change from the web client evaluates again too.
            if let Some(r) = policy.rules.first() {
                let version = s.read(active).unwrap().version;
                let Edited::Done { policy, .. } =
                    change(&s, version, Change::Remove(r.id), NOW).unwrap()
                else {
                    panic!("seed {seed} step {step}: remove");
                };
                let want = expected(&policy, &mail);
                for ((id, g), (_, w)) in stored(&s).iter().zip(want) {
                    assert!(
                        agrees(g, w, &policy),
                        "seed {seed} step {step} item {id} after remove"
                    );
                }
            }
        }
    }
}
