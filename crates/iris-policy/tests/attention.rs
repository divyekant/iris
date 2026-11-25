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
/// The labels: the name, and the provider ID. `SENT` makes mail from you.
const LABELS: [(&str, &str); 4] = [
    ("INBOX", "INBOX"),
    ("Promotions", "Label_1"),
    ("Work", "Label_2"),
    ("Sent", "SENT"),
];
const FORMS: [&str; 5] = ["commitment", "conversation", "record", "feed", "signal"];
const TYPES: [&str; 3] = ["receipt", "ticket", "other"];
const KINDS: [&str; 3] = ["task", "reply", "payment"];

/// One message of the generated mailbox, with what the policy sees.
struct Msg {
    sender: String,
    kind: SenderKind,
    /// The message has the SENT or DRAFT label.
    own: bool,
    hostile: bool,
    /// The message waits for its link check (a `link_check` row, store
    /// migration 20): it counts as hostile mail.
    pending: bool,
    labels: Vec<String>,
    /// `None`: the message has no date.
    date: Option<i64>,
    /// The date of the message row, which counts when there is no header date.
    internal: Option<i64>,
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
            for (i, (name, provider)) in LABELS.iter().enumerate() {
                tx.execute(
                    "INSERT INTO label(id, account_id, provider_id, name, kind) VALUES (?1, 1, ?2, ?3, 'user')",
                    rusqlite::params![i as i64 + 1, provider, name],
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
                let mut sent = false;
                for (i, (name, provider)) in LABELS.iter().enumerate() {
                    if rng.coin() {
                        tx.execute(
                            "INSERT INTO message_label(message_id, label_id) VALUES (?1, ?2)",
                            [id, i as i64 + 1],
                        )?;
                        labels.push(name.to_string());
                        sent |= *provider == "SENT";
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
                let internal = if in_header { None } else { Some(date) };
                // With no header, a date only in the header is no date.
                let date = if addr.is_empty() && in_header { None } else { Some(date) };
                out.push(Msg {
                    sender: addr.to_lowercase(),
                    kind: match addr {
                        _ if sent => SenderKind::SelfOwner,
                        "News@Shop.example" => SenderKind::Stranger,
                        _ => k.and_then(SenderKind::from_name).unwrap_or(SenderKind::Stranger),
                    },
                    own: sent,
                    hostile,
                    pending: false,
                    labels,
                    date,
                    internal,
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
                rng.pick(&["self", "person", "service", "stranger", "hostile"])
            ));
        }
        if rng.next().is_multiple_of(3) {
            when.push(format!(
                "sender = \"{}\"",
                rng.pick(&["@shop.example", "ann@friends.example"])
            ));
        }
        if rng.next().is_multiple_of(3) {
            when.push(format!("label = \"{}\"", rng.pick(&LABELS).0));
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
        // A name with a quote and a backslash, and at times one so long
        // that the reason gets cut.
        let long = if rng.next().is_multiple_of(3) {
            "x".repeat(250)
        } else {
            String::new()
        };
        s += &format!(
            "[[rule]]\nid = {}\nname = \"Rule \\\"{}\\\" a\\\\b{}\"\neffect = \"{}\"\nwhen = {{ {} }}\nby = \"person\"\nadded = 2026-11-10\n",
            ids,
            ids,
            long,
            rng.pick(&["hide", "bundle"]),
            when.join(", ")
        );
    }
    s
}

/// The `attention` field of each item, by item ID: the state, the rule, the
/// reason, and the number of keys.
type Field = (Option<String>, Option<u32>, Option<String>, i64);
fn stored(store: &Store) -> Vec<(i64, Field)> {
    store
        .read(|c| {
            c.prepare(
                "SELECT id, json_extract(doc, '$.attention.state'), json_extract(doc, '$.attention.rule'),
                        json_extract(doc, '$.attention.reason'),
                        (SELECT count(*) FROM json_each(doc, '$.attention'))
                 FROM item ORDER BY id",
            )?
            .query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))))?
                .collect::<rusqlite::Result<_>>()
                .map_err(Into::into)
        })
        .unwrap()
}

/// The attention of each item at the time `now`. A message with no date
/// has the age 0.
fn expected(policy: &Policy, mailbox: &[Msg], now: i64) -> Vec<(i64, Attention)> {
    let mut out = Vec::new();
    for m in mailbox {
        let all: Vec<Item> = m.items.iter().map(|(_, i)| *i).collect();
        let msg = Message {
            sender: &m.sender,
            sender_kind: m.kind,
            hostile: m.hostile || m.pending,
            labels: &m.labels,
            date: m.date.unwrap_or(now),
            items: &all,
        };
        for (id, it) in &m.items {
            out.push((*id, policy.attention(it, &msg, now)));
        }
    }
    out.sort_by_key(|(id, _)| *id);
    out
}

/// The `attention` field of the schema for `want`.
fn agrees(got: &Field, want: Attention, policy: &Policy) -> bool {
    let (state, rule) = match want {
        Attention::Shown => return *got == (Some("shown".into()), None, None, 1),
        Attention::Hidden { rule } => ("hidden", rule),
        Attention::Bundled { rule } => ("bundled", rule),
    };
    let name = &policy.rules.iter().find(|r| r.id == rule).unwrap().name;
    let verb = if state == "hidden" {
        "Hidden"
    } else {
        "Bundled"
    };
    let full: Vec<char> = format!("{verb} by rule {rule}: {name}").chars().collect();
    let reason = if full.len() > 200 {
        full[..197].iter().collect::<String>() + "..."
    } else {
        full.iter().collect()
    };
    *got == (Some(state.into()), Some(rule), Some(reason), 3)
}

#[test]
fn each_policy_change_writes_the_attention_of_each_item() {
    // Invariant: after each confirmation and each change from the web
    // client, the attention of each item is the attention that the active
    // policy gives it.
    // An item whose attention changes gets the time of the change in
    // `updated_at` (milliseconds) and `doc.updated`; each other item keeps
    // its times.
    let check =
        |s: &Store, policy: &Policy, mail: &[Msg], t: i64, before: &[(i64, Field)], what: &str| {
            let times = stamps(s);
            let got = stored(s);
            let want = expected(policy, mail, t);
            assert_eq!(got.len(), want.len());
            for (((id, g), (_, w)), ((_, old), (_, (at, updated)))) in
                got.iter().zip(want).zip(before.iter().zip(times))
            {
                assert!(agrees(g, w, policy), "{what} item {id}: {g:?} != {w:?}");
                if g != old {
                    assert_eq!(
                        (at, updated.as_deref()),
                        (t * 1000, Some(&*iso(s, t))),
                        "{what} item {id}"
                    );
                } else {
                    assert!(
                        at < t * 1000,
                        "{what} item {id}: an item with no change got a new time"
                    );
                }
            }
        };
    for seed in 1..=20u64 {
        let (_d, s) = store(&format!("p{seed}"));
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15));
        let mail = mailbox(&s, &mut rng);
        let mut ids = 0;
        for step in 0..4i64 {
            let t = NOW + step * 3600;
            let text = format!("# step {step}\n{}", random_policy(&mut rng, &mut ids));
            std::fs::write(s.dir().join(FILE), &text).unwrap();
            let sha = match file_state(&s).unwrap() {
                FileState::Changed(l) => l.sha256,
                other => panic!("seed {seed} step {step}: {other:?}"),
            };
            let before = stored(&s);
            let policy = match confirm(&s, &sha, t).unwrap() {
                Confirm::Done { policy, .. } => policy,
                other => panic!("seed {seed} step {step}: {other:?}"),
            };
            check(
                &s,
                &policy,
                &mail,
                t,
                &before,
                &format!("seed {seed} step {step}\n{text}\n"),
            );
            // A change from the web client evaluates again too.
            if let Some(r) = policy.rules.first() {
                let version = s.read(active).unwrap().version;
                let before = stored(&s);
                let Edited::Done { policy, .. } =
                    change(&s, version, Change::Remove(r.id), t + 60).unwrap()
                else {
                    panic!("seed {seed} step {step}: remove");
                };
                check(
                    &s,
                    &policy,
                    &mail,
                    t + 60,
                    &before,
                    &format!("seed {seed} step {step} after remove"),
                );
            }
        }
    }
}

/// `updated_at` and `doc.updated` of each item, by item ID.
fn stamps(store: &Store) -> Vec<(i64, (i64, Option<String>))> {
    store
        .read(|c| {
            c.prepare(
                "SELECT id, updated_at, json_extract(doc, '$.updated') FROM item ORDER BY id",
            )?
            .query_map([], |r| Ok((r.get(0)?, (r.get(1)?, r.get(2)?))))?
            .collect::<rusqlite::Result<_>>()
            .map_err(Into::into)
        })
        .unwrap()
}

/// The time `t` (seconds) in the form of the open model: RFC 3339, UTC, with
/// milliseconds.
fn iso(_: &Store, t: i64) -> String {
    let (days, secs) = (t.div_euclid(86_400), t.rem_euclid(86_400));
    // 1_800_000_000 is 2027-01-15T08:00:00Z; the test times are on that day.
    assert_eq!(days, 20_833, "the test times must stay on 2027-01-15");
    format!(
        "2027-01-15T{:02}:{:02}:{:02}.000Z",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60
    )
}

/// Activates the policy `text` at the time `t`.
fn activate(s: &Store, text: &str, t: i64) -> Policy {
    std::fs::write(s.dir().join(FILE), text).unwrap();
    let sha = match file_state(s).unwrap() {
        FileState::Changed(l) => l.sha256,
        other => panic!("{other:?}"),
    };
    match confirm(s, &sha, t).unwrap() {
        Confirm::Done { policy, .. } => policy,
        other => panic!("{other:?}"),
    }
}

/// The messages in `attention_due`.
fn due(s: &Store) -> Vec<i64> {
    s.read(|c| {
        c.prepare("SELECT message_id FROM attention_due ORDER BY message_id")?
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()
            .map_err(Into::into)
    })
    .unwrap()
}

fn sql(s: &Store, q: &str, p: impl rusqlite::Params) {
    s.write(|tx| {
        tx.execute(q, p)?;
        Ok(())
    })
    .unwrap();
}

/// The message IDs of the mailbox are 1, 2, ... in the order of `mail`.
fn msg_id(i: usize) -> i64 {
    i as i64 + 1
}

/// The kind of each sender row that the test changes (the lowercase address).
const MOVABLE: [&str; 4] = [
    "ann@friends.example",
    "deals@shop.example",
    "x@far.example",
    "unknown@else.example",
];
const KIND_NAMES: [&str; 4] = ["person", "service", "stranger", "hostile"];

#[test]
fn each_fact_change_gets_the_attention_of_the_policy_after_a_refresh() {
    // Invariant: after `refresh_attention`, the attention of each item is
    // the attention that the active policy gives it, also after a change
    // of a fact of its origin message: a label, a label name, the hostile
    // marks, a link check that starts or ends, the date, the kind of the
    // sender, a new item of the message,
    // or a new form, type or document of the item. Each such change marks
    // the message in `attention_due` (store migration 8).
    for seed in 1..=20u64 {
        let (_d, s) = store(&format!("r{seed}"));
        let mut rng = Rng(seed.wrapping_mul(0xD1B5_4A32_D192_ED03));
        let mut mail = mailbox(&s, &mut rng);
        // Each new item marks its message.
        assert_eq!(due(&s).len(), mail.len(), "seed {seed}: new items");
        // A store from before migration 8 has items and no mark: the
        // first refresh evaluates each item.
        sql(&s, "DELETE FROM attention_due", []);
        // With no policy, each item is shown.
        assert_eq!(refresh_attention(&s, NOW).unwrap(), mail.len() as u64);
        assert!(stored(&s)
            .iter()
            .all(|(_, f)| f.0.as_deref() == Some("shown")));
        let mut ids = 0;
        let text = format!("# r\n{}", random_policy(&mut rng, &mut ids));
        let policy = activate(&s, &text, NOW);
        assert!(
            due(&s).is_empty(),
            "seed {seed}: a confirmation evaluates each item"
        );
        let mut kinds: Vec<(&str, Option<SenderKind>)> = MOVABLE
            .iter()
            .map(|a| {
                let k = SENDERS
                    .iter()
                    .find(|(x, _)| x.to_lowercase() == *a)
                    .unwrap()
                    .1;
                (*a, k.and_then(SenderKind::from_name))
            })
            .collect();
        // One time for each round: only the `all` pass follows the time.
        let t = NOW;
        let mut names: Vec<String> = LABELS.iter().map(|l| l.0.to_string()).collect();
        for round in 0..8 {
            let mut what = format!("seed {seed} round {round}\n{text}");
            let mut marked = Vec::new();
            for _ in 0..1 + rng.next() % 3 {
                let i = (rng.next() % mail.len() as u64) as usize;
                let id = msg_id(i);
                let op = rng.next() % 11;
                what += &format!("\nop {op} on message {id}");
                match op {
                    // A label on or off.
                    0 => {
                        let l = (rng.next() % LABELS.len() as u64) as usize;
                        let name = names[l].clone();
                        let m = &mut mail[i];
                        if let Some(p) = m.labels.iter().position(|x| *x == name) {
                            sql(
                                &s,
                                "DELETE FROM message_label WHERE message_id = ?1 AND label_id = ?2",
                                [id, l as i64 + 1],
                            );
                            m.labels.remove(p);
                        } else {
                            sql(
                                &s,
                                "INSERT OR FAIL INTO message_label(message_id, label_id) VALUES (?1, ?2)",
                                [id, l as i64 + 1],
                            );
                            m.labels.push(name);
                        }
                        m.own = m.labels.iter().any(|x| x == "Sent");
                        marked.push(id);
                    }
                    // The hostile marks.
                    1 => {
                        let m = &mut mail[i];
                        m.hostile = !m.hostile;
                        let v = if m.hostile { Some("[\"spoof\"]") } else { None };
                        sql(
                            &s,
                            "UPDATE OR ROLLBACK message SET hostile = ?2 WHERE id = ?1",
                            rusqlite::params![id, v],
                        );
                        marked.push(id);
                    }
                    // The date: of the header if the message has one.
                    2 => {
                        let d = NOW - (rng.next() % 60) as i64 * DAY;
                        let m = &mut mail[i];
                        if m.sender.is_empty() {
                            sql(
                                &s,
                                "UPDATE message SET internal_date = ?2 WHERE id = ?1",
                                [id, d * 1000],
                            );
                            m.internal = Some(d);
                        } else {
                            sql(
                                &s,
                                "UPDATE header SET date = ?2 WHERE message_id = ?1",
                                [id, d * 1000],
                            );
                        }
                        m.date = Some(d);
                        marked.push(id);
                    }
                    // The kind of a sender: an update, a new row or no row.
                    3 => {
                        let k = (rng.next() % MOVABLE.len() as u64) as usize;
                        let (addr, old) = kinds[k];
                        let new = if rng.next().is_multiple_of(4) {
                            None
                        } else {
                            Some(*rng.pick(&KIND_NAMES))
                        };
                        match (old, new) {
                            // An upsert, as the sender axis writes: its conflict
                            // clause also applies to the statements of the triggers.
                            (_, Some(n)) => sql(
                                &s,
                                "INSERT INTO sender(address, kind) VALUES (?1, ?2)
                                 ON CONFLICT(address) DO UPDATE SET kind = excluded.kind",
                                [addr, n],
                            ),
                            (Some(_), None) => {
                                sql(&s, "DELETE FROM sender WHERE address = ?1", [addr])
                            }
                            (None, None) => {}
                        }
                        kinds[k].1 = new.and_then(SenderKind::from_name);
                        for (j, m) in mail.iter().enumerate() {
                            if m.sender == addr && old != kinds[k].1 {
                                marked.push(msg_id(j));
                            }
                        }
                    }
                    // A new type of a record or kind of a commitment, or a
                    // new document with no attention, as a rewrite of a
                    // conversation gives.
                    4 | 5 => {
                        let m = &mut mail[i];
                        let n = (rng.next() % m.items.len() as u64) as usize;
                        let (item, it) = &mut m.items[n];
                        let item = *item;
                        let (doc, rt, ck) = match it.form {
                            Form::Record => {
                                let t = *rng.pick(&TYPES);
                                (
                                    format!(r#"{{"type":"{t}"}}"#),
                                    RecordType::from_name(t),
                                    None,
                                )
                            }
                            Form::Commitment => {
                                let c = *rng.pick(&KINDS);
                                (
                                    format!(r#"{{"kind":"{c}"}}"#),
                                    None,
                                    CommitmentKind::from_name(c),
                                )
                            }
                            _ => ("{}".to_string(), None, None),
                        };
                        let before = stored(&s);
                        sql(
                            &s,
                            "UPDATE item SET doc = ?2 WHERE id = ?1",
                            rusqlite::params![item, doc],
                        );
                        // The old attention stays until the refresh.
                        assert_eq!(
                            stored(&s).iter().find(|(x, _)| *x == item),
                            before.iter().find(|(x, _)| *x == item),
                            "{what}: item {item} lost its attention"
                        );
                        it.record_type = rt;
                        it.commitment_kind = ck;
                        marked.push(id);
                    }
                    // A new item of the message.
                    6 => {
                        sql(&s, "INSERT INTO item(form, account_id, status, doc, created_at, updated_at) VALUES ('feed', 1, 'new', '{}', 0, 0)", []);
                        let item: i64 = s
                            .read(
                                |c| Ok(c.query_row("SELECT max(id) FROM item", [], |r| r.get(0))?),
                            )
                            .unwrap();
                        sql(&s, "INSERT INTO item_source(item_id, message_id, role) VALUES (?1, ?2, 'origin')", [item, id]);
                        mail[i].items.push((
                            item,
                            Item {
                                form: Form::Feed,
                                record_type: None,
                                commitment_kind: None,
                            },
                        ));
                        marked.push(id);
                    }
                    // A new origin of an item, as a conversation gets when
                    // its origin becomes hostile mail: both messages.
                    7 => {
                        let j = (rng.next() % mail.len() as u64) as usize;
                        if j != i && mail[i].items.len() > 1 {
                            let (item, it) = mail[i].items.remove(0);
                            sql(&s, "UPDATE item_source SET message_id = ?2 WHERE item_id = ?1 AND role = 'origin'", [item, msg_id(j)]);
                            mail[j].items.push((item, it));
                            marked.push(msg_id(j));
                        }
                    }
                    // A header that comes after the items, or the cleanup of
                    // gone mail that deletes the header.
                    8 => {
                        let m = &mut mail[i];
                        if m.sender.is_empty() {
                            let d = NOW - (rng.next() % 60) as i64 * DAY;
                            sql(&s, "INSERT INTO header(message_id, date, from_addr) VALUES (?1, ?2, 'ann@friends.example')", [id, d * 1000]);
                            m.sender = "ann@friends.example".into();
                            m.date = Some(d);
                        } else {
                            sql(&s, "DELETE FROM header WHERE message_id = ?1", [id]);
                            m.sender = String::new();
                            m.date = m.internal;
                        }
                        marked.push(id);
                    }
                    // The link check of the message starts or ends.
                    10 => {
                        let m = &mut mail[i];
                        m.pending = !m.pending;
                        let q = if m.pending {
                            "INSERT INTO link_check(message_id) VALUES (?1)"
                        } else {
                            "DELETE FROM link_check WHERE message_id = ?1"
                        };
                        sql(&s, q, [id]);
                        marked.push(id);
                    }
                    // A new name of a label: each message with it.
                    _ => {
                        let l = 1 + (rng.next() % 2) as usize;
                        let old = names[l].clone();
                        let new = if old == LABELS[l].0 {
                            format!("Renamed {l}")
                        } else {
                            LABELS[l].0.to_string()
                        };
                        names[l] = new.clone();
                        sql(
                            &s,
                            "UPDATE label SET name = ?2 WHERE id = ?1",
                            rusqlite::params![l as i64 + 1, new],
                        );
                        for (j, m) in mail.iter_mut().enumerate() {
                            if let Some(x) = m.labels.iter_mut().find(|x| **x == old) {
                                if s.read(|c| Ok(c.query_row(
                                    "SELECT count(*) FROM message_label WHERE message_id = ?1 AND label_id = ?2",
                                    [msg_id(j), l as i64 + 1], |r| r.get::<_, i64>(0))?)).unwrap() == 1
                                {
                                    *x = new.clone();
                                    marked.push(msg_id(j));
                                }
                            }
                        }
                    }
                }
            }
            for m in mail.iter_mut() {
                m.kind = if m.own {
                    SenderKind::SelfOwner
                } else if m.sender == "news@shop.example" {
                    SenderKind::Stranger
                } else {
                    kinds
                        .iter()
                        .find(|(a, _)| *a == m.sender)
                        .and_then(|(_, k)| *k)
                        .unwrap_or(SenderKind::Stranger)
                };
            }
            let now_due = due(&s);
            for id in &marked {
                assert!(now_due.contains(id), "{what}: message {id} is not marked");
            }
            refresh_attention(&s, t).unwrap();
            assert!(due(&s).is_empty(), "{what}: a refresh clears each mark");
            for ((id, g), (_, w)) in stored(&s).iter().zip(expected(&policy, &mail, t)) {
                assert!(agrees(g, w, &policy), "{what} item {id}: {g:?} != {w:?}");
            }
        }
        // The attention write marks nothing.
        sql(&s, "UPDATE item SET updated_at = 5, doc = json_set(doc, '$.attention', json('{\"state\":\"hidden\"}'), '$.updated', 'x')", []);
        assert!(
            due(&s).is_empty(),
            "seed {seed}: the attention write marked a message"
        );
        // Each hour, each item, also with no mark: a rule with
        // `older_than_days` matches more items as time goes on.
        assert_eq!(refresh_attention(&s, t + PASS_EVERY - 1).unwrap(), 0);
        let later = NOW + 40 * DAY;
        assert_eq!(refresh_attention(&s, later).unwrap(), mail.len() as u64);
        assert_eq!(refresh_attention(&s, later + 60).unwrap(), 0);
        for ((id, g), (_, w)) in stored(&s).iter().zip(expected(&policy, &mail, later)) {
            assert!(
                agrees(g, w, &policy),
                "seed {seed} later item {id}: {g:?} != {w:?}"
            );
        }
    }
}

/// Property: the header and the sender rows use one address form, so each
/// change of a sender row marks each message of that sender, whatever the
/// case of the address in the header or in the row (issue 166).
#[test]
fn a_sender_change_marks_the_header_in_each_case_form() {
    let (_d, s) = store("sender-case");
    sql(
        &s,
        "INSERT INTO account(id, provider, address) VALUES (1, 'gmail', 'me@x.example')",
        [],
    );
    sql(
        &s,
        "INSERT INTO thread(account_id, provider_id) VALUES (1, 't')",
        [],
    );
    let forms = [
        "news@shop.example",
        "News@Shop.example",
        "NEWS@SHOP.EXAMPLE",
        "nEwS@sHoP.ExAmPlE",
    ];
    for (m, header) in forms.iter().enumerate() {
        let id = m as i64 + 1;
        sql(
            &s,
            "INSERT INTO message(id, account_id, thread_id, provider_id) VALUES (?1, 1, 1, ?2)",
            rusqlite::params![id, format!("m{m}")],
        );
        // Each write path of a header: an insert, and an update.
        if m % 2 == 0 {
            sql(
                &s,
                "INSERT INTO header(message_id, from_addr) VALUES (?1, ?2)",
                rusqlite::params![id, header],
            );
        } else {
            sql(
                &s,
                "INSERT INTO header(message_id, from_addr) VALUES (?1, 'x@y.example')",
                [id],
            );
            sql(
                &s,
                "UPDATE header SET from_addr = ?2 WHERE message_id = ?1",
                rusqlite::params![id, header],
            );
        }
        sql(
            &s,
            "INSERT INTO item(id, form, account_id, status, doc, created_at, updated_at)
             VALUES (?1, 'feed', 1, 'new', '{}', 0, 0)",
            [id],
        );
        sql(
            &s,
            "INSERT INTO item_source(item_id, message_id, role) VALUES (?1, ?1, 'origin')",
            [id],
        );
    }
    let all: Vec<i64> = (1..=forms.len() as i64).collect();
    for row in forms {
        for (step, q) in [
            "INSERT INTO sender(address, kind) VALUES (?1, 'service')",
            "UPDATE sender SET kind = 'hostile' WHERE address = ?1",
            "DELETE FROM sender WHERE address = ?1",
        ]
        .iter()
        .enumerate()
        {
            sql(&s, "DELETE FROM attention_due", []);
            sql(&s, q, [row]);
            assert_eq!(due(&s), all, "row {row}, step {step}");
        }
    }
}

/// Migration 39 changes a mixed-case `from_addr` of an older store to the
/// one form, so a later change of the sender row marks the message.
#[test]
fn migration_39_lowercases_the_headers_of_an_older_store() {
    let dir = std::env::temp_dir().join(format!("iris-attn-m39-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let _d = Dir(dir.clone());
    {
        let s = Store::open(&dir).unwrap();
        // A store of migration 38, with a header in another case.
        s.write(|tx| {
            tx.execute_batch(
                "DROP TRIGGER header_from_addr_lower_add; DROP TRIGGER header_from_addr_lower; DROP TRIGGER talk_due_sender_add; DROP TRIGGER talk_due_sender; DROP TRIGGER talk_due_sender_remove; DROP TRIGGER model_declined_end; DROP TRIGGER model_declined_end_remove; DROP TABLE model_declined; DROP TRIGGER IF EXISTS context_gone; DROP TRIGGER IF EXISTS context_link_check_add; DROP TRIGGER IF EXISTS context_link_check_end; ALTER TABLE message DROP COLUMN context; ALTER TABLE reply_draft DROP COLUMN seen; DROP TRIGGER IF EXISTS spam_drops_items; DROP TRIGGER IF EXISTS spam_back_drops_items; DROP TRIGGER IF EXISTS understand_reset; DROP TRIGGER IF EXISTS understand_new_content; DROP TRIGGER IF EXISTS understand_come_back; DROP TRIGGER understand_owner_label_add; DROP TRIGGER understand_owner_label_remove; CREATE TRIGGER understand_owner_label_add AFTER INSERT ON message_label WHEN (SELECT provider_id FROM label WHERE id = NEW.label_id) IN ('SENT', 'DRAFT') BEGIN UPDATE message SET understood_at = NULL WHERE id = NEW.message_id AND understood_at IS NOT NULL; END; CREATE TRIGGER understand_owner_label_remove AFTER DELETE ON message_label WHEN (SELECT provider_id FROM label WHERE id = OLD.label_id) IN ('SENT', 'DRAFT') BEGIN UPDATE message SET understood_at = NULL WHERE id = OLD.message_id AND understood_at IS NOT NULL; END; DROP TABLE talk_due; DROP TRIGGER sent_copy_keep; DROP TRIGGER sent_copy_add; DROP VIEW iris_sent_copy; DROP INDEX draft_sent_copy; DROP TRIGGER draft_or_copy_drops_items; DROP TRIGGER draft_or_copy_talk_add; DROP TRIGGER draft_or_copy_talk_remove; DROP TRIGGER draft_or_copy_back_drops_items; DROP VIEW item_droppable;
                 DROP TRIGGER changeset_withdraw_by; DROP TRIGGER changeset_withdrawn_by; ALTER TABLE changeset DROP COLUMN withdrawn_by; DROP TRIGGER matter_add_end; DROP TRIGGER matter_item_kept_add; DROP TRIGGER matter_item_kept_update; DROP TRIGGER matter_add_insert; DROP TRIGGER matter_add_update; DROP VIEW matter_add_live; DROP TABLE matter_add; DROP TRIGGER matter_insert; DROP TRIGGER matter_update; DROP TRIGGER matter_decide; DROP TRIGGER matter_item_insert; DROP TRIGGER matter_item_update; DROP TRIGGER matter_delete; ALTER TABLE matter DROP COLUMN author; DROP TRIGGER ledger_hostile_before; ALTER TABLE ledger DROP COLUMN hostile_before; DROP TRIGGER fixed_due_release; DROP TRIGGER message_release_content; ALTER TABLE message DROP COLUMN released; ALTER TABLE message DROP COLUMN fixed_rules; DROP TRIGGER sender_gate_rev; ALTER TABLE sender DROP COLUMN gate_rev; PRAGMA user_version = 38;
                 INSERT INTO account(id, provider, address) VALUES (1, 'gmail', 'me@x.example');
                 INSERT INTO thread(account_id, provider_id) VALUES (1, 't');
                 INSERT INTO message(id, account_id, thread_id, provider_id) VALUES (1, 1, 1, 'm');
                 INSERT INTO header(message_id, from_addr) VALUES (1, 'News@Shop.example');
                 INSERT INTO item(id, form, account_id, status, doc, created_at, updated_at)
                   VALUES (1, 'feed', 1, 'new', '{}', 0, 0);
                 INSERT INTO item_source(item_id, message_id, role) VALUES (1, 1, 'origin');",
            )?;
            Ok(())
        })
        .unwrap();
        let stored: String = s
            .read(|c| Ok(c.query_row("SELECT from_addr FROM header", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(stored, "News@Shop.example");
    }
    let s = Store::open(&dir).unwrap();
    let stored: String = s
        .read(|c| Ok(c.query_row("SELECT from_addr FROM header", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(stored, "news@shop.example");
    sql(&s, "DELETE FROM attention_due", []);
    sql(
        &s,
        "INSERT INTO sender(address, kind) VALUES ('news@shop.example', 'service')",
        [],
    );
    assert_eq!(due(&s), vec![1]);
}
