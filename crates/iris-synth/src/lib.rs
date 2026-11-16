//! A deterministic synthetic mailbox for Iris tests.
//!
//! `Mailbox::new(seed, count)` makes a plan of `count` messages in threads over
//! two years. `Mailbox::raw(i)` renders message `i` as RFC 5322 bytes. The same
//! seed and count give the same bytes on each run and on each platform.
//!
//! The true form and sender kind of each message are in `Mailbox::truth(i)`,
//! never in the mail. All names are made from syllables. All addresses use the
//! reserved `example.com`, `example.org` and `example.net` domains.

pub mod base64;
mod date;
mod rng;
mod text;

use rng::Rng;

/// The default size of the synthetic mailbox.
pub const DEFAULT_COUNT: usize = 100_000;

/// The address of the owner of the synthetic mailbox.
pub const OWNER: &str = "owner@example.com";
const OWNER_NAME: &str = "Avel Torun";

/// 2026-01-01T00:00:00Z. The mailbox ends at this time.
const END: i64 = 1_767_225_600;
/// 2024-01-01T00:00:00Z. The mailbox starts at this time.
const START: i64 = 1_704_067_200;
const SPAN: i64 = END - START;

/// The five forms of a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Form {
    Commitment,
    Conversation,
    Record,
    Feed,
    Signal,
}

impl Form {
    pub const ALL: [Form; 5] = [
        Form::Commitment,
        Form::Conversation,
        Form::Record,
        Form::Feed,
        Form::Signal,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Form::Commitment => "commitment",
            Form::Conversation => "conversation",
            Form::Record => "record",
            Form::Feed => "feed",
            Form::Signal => "signal",
        }
    }
}

/// The kind of the sender of a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Sender {
    /// The owner of the mailbox. The message is in the sent mail.
    Owner,
    KnownPerson,
    KnownService,
    Stranger,
    Hostile,
}

impl Sender {
    pub fn name(self) -> &'static str {
        match self {
            Sender::Owner => "owner",
            Sender::KnownPerson => "known_person",
            Sender::KnownService => "known_service",
            Sender::Stranger => "stranger",
            Sender::Hostile => "hostile",
        }
    }
}

/// Hard samples in a message. One message can have more than one.
pub mod traits {
    pub const PROMPT_INJECTION: u8 = 1;
    pub const PHISHING_LINK: u8 = 2;
    pub const SPOOFED_NAME: u8 = 4;
    pub const ODD_ENCODING: u8 = 8;
    pub const BIG_ATTACHMENT: u8 = 16;

    pub const NAMES: [(u8, &str); 5] = [
        (PROMPT_INJECTION, "prompt_injection"),
        (PHISHING_LINK, "phishing_link"),
        (SPOOFED_NAME, "spoofed_display_name"),
        (ODD_ENCODING, "odd_encoding"),
        (BIG_ATTACHMENT, "big_attachment"),
    ];
}

/// The plan of one message. The body is rendered from the seed on request.
#[derive(Clone, Copy, Debug)]
struct Meta {
    thread: u32,
    pos: u16,
    date: i64,
    form: Form,
    sender: Sender,
    /// The other side of the thread is a stranger, not a known person.
    stranger_thread: bool,
    /// The true kind of the sender, by the rules of spec 1 section 6.7. It is
    /// `sender`, except for a stranger that the owner wrote to in the thread:
    /// that is a known person. The kind belongs to the address, so the first
    /// messages of a service are from a known service too.
    truth: Sender,
    traits: u8,
}

/// The true description of one message, for tests and evaluation only.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Truth {
    pub index: usize,
    pub message_id: String,
    pub thread: u32,
    pub date: i64,
    pub form: Form,
    pub sender: Sender,
    pub traits: u8,
}

impl Truth {
    /// One JSON line for the ground-truth file. All strings are plain ASCII.
    pub fn json_line(&self) -> String {
        let traits: Vec<String> = traits::NAMES
            .iter()
            .filter(|(bit, _)| self.traits & bit != 0)
            .map(|(_, name)| format!("\"{name}\""))
            .collect();
        format!(
            "{{\"index\":{},\"message_id\":\"{}\",\"thread\":{},\"date\":{},\"form\":\"{}\",\"sender\":\"{}\",\"traits\":[{}]}}",
            self.index,
            self.message_id,
            self.thread,
            self.date,
            self.form.name(),
            self.sender.name(),
            traits.join(",")
        )
    }
}

/// A synthetic mailbox: a plan of messages, sorted by date, oldest first.
pub struct Mailbox {
    seed: u64,
    metas: Vec<Meta>,
}

impl Mailbox {
    pub fn new(seed: u64, count: usize) -> Mailbox {
        let mut metas = Vec::with_capacity(count + 8);
        let mut thread: u32 = 0;
        while metas.len() < count {
            plan_thread(seed, thread, &mut metas);
            thread += 1;
        }
        metas.truncate(count);
        metas.sort_by_key(|m| (m.date, m.thread, m.pos));
        let owner_threads: std::collections::HashSet<u32> = metas
            .iter()
            .filter(|m| m.sender == Sender::Owner)
            .map(|m| m.thread)
            .collect();
        for m in &mut metas {
            if m.sender == Sender::Stranger && owner_threads.contains(&m.thread) {
                m.truth = Sender::KnownPerson;
            }
        }
        Mailbox { seed, metas }
    }

    pub fn len(&self) -> usize {
        self.metas.len()
    }

    pub fn is_empty(&self) -> bool {
        self.metas.is_empty()
    }

    pub fn truth(&self, i: usize) -> Truth {
        let m = &self.metas[i];
        Truth {
            index: i,
            message_id: message_id(m.thread, m.pos),
            thread: m.thread,
            date: m.date,
            form: m.form,
            sender: m.truth,
            traits: m.traits,
        }
    }

    /// The true fields of the item that message `i` makes, for evaluation
    /// only. Keys are dotted paths of the open model, for example
    /// `fields.amount.value` of a record or `due.date` of a commitment. A
    /// receipt and the first message of a commitment thread have fields. Each
    /// other message gives an empty list.
    pub fn fields(&self, i: usize) -> Vec<(&'static str, String)> {
        text::fields(self.seed, &self.metas[i])
    }

    /// Render message `i` as RFC 5322 bytes with CRLF line ends.
    pub fn raw(&self, i: usize) -> Vec<u8> {
        text::render(self.seed, &self.metas[i])
    }
}

fn message_id(thread: u32, pos: u16) -> String {
    format!("<t{thread}.{pos}@synth.example.com>")
}

fn plan_thread(seed: u64, thread: u32, out: &mut Vec<Meta>) {
    let mut r = Rng::stream(seed, thread, 0, b'p');
    let mut traits = 0u8;
    let (form, sender) = if r.chance(30) {
        // About 3 % of threads come from a hostile sender.
        let form = *r.pick(&[Form::Signal, Form::Record, Form::Commitment]);
        traits |= *r.pick(&[
            traits::PROMPT_INJECTION,
            traits::PHISHING_LINK,
            traits::SPOOFED_NAME,
            traits::PHISHING_LINK | traits::SPOOFED_NAME,
            traits::PROMPT_INJECTION | traits::SPOOFED_NAME,
        ]);
        (form, Sender::Hostile)
    } else {
        match r.below(100) {
            0..=29 => (Form::Feed, Sender::KnownService),
            30..=54 => (Form::Signal, Sender::KnownService),
            55..=69 => (Form::Record, Sender::KnownService),
            70..=89 => {
                let s = if r.chance(100) {
                    Sender::Stranger
                } else {
                    Sender::KnownPerson
                };
                (Form::Conversation, s)
            }
            _ => {
                let s = if r.chance(150) {
                    Sender::Stranger
                } else {
                    Sender::KnownPerson
                };
                (Form::Commitment, s)
            }
        }
    };
    let len = match (form, sender) {
        (_, Sender::Hostile) => 1,
        (Form::Conversation, _) => 1 + r.below(4) + r.below(4),
        (Form::Commitment, _) => 1 + r.below(4),
        _ => 1,
    } as u16;
    // A few threads start with the owner (sent mail first).
    let owner_first = len > 1 && r.chance(200);
    let mut date = END - SPAN + r.below(SPAN as u64) as i64;
    for pos in 0..len {
        if date >= END {
            break;
        }
        let mut t = traits;
        let mut m_rng = Rng::stream(seed, thread, pos, b'm');
        if m_rng.below(100) == 0 {
            t |= traits::ODD_ENCODING;
        }
        if m_rng.below(4000) == 0 {
            t |= traits::BIG_ATTACHMENT;
        }
        let from_owner = (pos % 2 == 1) != owner_first;
        let owner = sender != Sender::Hostile && len > 1 && from_owner;
        out.push(Meta {
            thread,
            pos,
            date,
            form,
            sender: if owner { Sender::Owner } else { sender },
            stranger_thread: sender == Sender::Stranger,
            truth: if owner { Sender::Owner } else { sender },
            traits: t,
        });
        date += 600 + r.below(2 * 86_400) as i64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// FNV-1a, 64 bit. Stable on every platform and every Rust version.
    fn fnv(h: &mut u64, bytes: &[u8]) {
        for b in bytes {
            *h ^= *b as u64;
            *h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn hash(seed: u64, count: usize) -> u64 {
        let mb = Mailbox::new(seed, count);
        let mut h = 0xcbf2_9ce4_8422_2325;
        for i in 0..mb.len() {
            fnv(&mut h, &mb.raw(i));
            fnv(&mut h, mb.truth(i).json_line().as_bytes());
        }
        h
    }

    #[test]
    fn same_seed_gives_same_bytes() {
        assert_eq!(hash(7, 3000), hash(7, 3000));
        assert_ne!(hash(7, 3000), hash(8, 3000));
        // The pinned value catches a change of output between platforms or
        // versions. If you change the generator on purpose, update the value.
        assert_eq!(hash(42, 2000), GOLDEN_42_2000, "got {:#x}", hash(42, 2000));
    }

    const GOLDEN_42_2000: u64 = 0x300f_c186_69b7_f8d7;

    #[test]
    fn full_mailbox_has_every_form_sender_and_trait() {
        let mb = Mailbox::new(1, DEFAULT_COUNT);
        assert_eq!(mb.len(), DEFAULT_COUNT);
        let mut forms = [0usize; 5];
        let mut senders = std::collections::BTreeMap::new();
        let mut trait_count = [0usize; 5];
        let mut threads = std::collections::BTreeSet::new();
        let mut multi = 0;
        for i in 0..mb.len() {
            let t = mb.truth(i);
            forms[Form::ALL.iter().position(|f| *f == t.form).unwrap()] += 1;
            *senders.entry(t.sender).or_insert(0usize) += 1;
            for (k, (bit, _)) in traits::NAMES.iter().enumerate() {
                if t.traits & bit != 0 {
                    trait_count[k] += 1;
                }
            }
            if !threads.insert(t.thread) {
                multi += 1;
            }
        }
        assert!(forms.iter().all(|n| *n > 1000), "forms {forms:?}");
        assert_eq!(senders.len(), 5, "senders {senders:?}");
        assert!(trait_count.iter().all(|n| *n > 5), "traits {trait_count:?}");
        assert!(multi > 10_000, "replies in threads: {multi}");
        let first = mb.truth(0).date;
        let last = mb.truth(mb.len() - 1).date;
        assert!(last - first > SPAN - 30 * 86_400 && last < END);
        assert!(
            (START..START + 86_400).contains(&first),
            "first date {first}"
        );
    }

    #[test]
    fn messages_are_rfc5322_shaped_and_use_reserved_domains() {
        let mb = Mailbox::new(3, 5000);
        for i in 0..mb.len() {
            let raw = mb.raw(i);
            let split = raw
                .windows(4)
                .position(|w| w == b"\r\n\r\n")
                .expect("header end");
            let head = String::from_utf8(raw[..split].to_vec()).expect("ASCII headers");
            for name in ["Message-ID: ", "Date: ", "From: ", "To: ", "Subject: "] {
                assert!(head.contains(name), "{name} missing in message {i}");
            }
            assert!(
                !raw.windows(2).any(|w| w[1] == b'\n' && w[0] != b'\r'),
                "bare LF in {i}"
            );
            for word in head.split(|c: char| c.is_whitespace() || "<>,;\"".contains(c)) {
                if let Some(at) = word.find('@') {
                    let domain = &word[at + 1..];
                    assert!(
                        ["example.com", "example.org", "example.net"]
                            .iter()
                            .any(|d| domain == *d || domain.ends_with(&format!(".{d}"))),
                        "domain {domain} in message {i}"
                    );
                }
            }
            assert!(!head.contains("form") && !head.contains("hostile"));
        }
    }

    #[test]
    fn every_mime_part_has_a_header_block() {
        let mb = Mailbox::new(1, 20_000);
        let big = (0..mb.len()).find(|i| mb.truth(*i).traits & traits::BIG_ATTACHMENT != 0);
        let raw = String::from_utf8_lossy(&mb.raw(big.expect("a big attachment"))).into_owned();
        let boundary = raw
            .split("boundary=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        let parts: Vec<&str> = raw.split(&format!("--{boundary}\r\n")).skip(1).collect();
        assert_eq!(parts.len(), 2);
        for part in parts {
            let head = &part[..part
                .find("\r\n\r\n")
                .expect("blank line after the part headers")];
            for line in head.split("\r\n") {
                let name = line.split(':').next().unwrap();
                assert!(
                    line.starts_with([' ', '\t']) || (line.contains(": ") && !name.contains(' ')),
                    "not a header line: {line:?}"
                );
            }
        }
    }

    #[test]
    fn encoded_words_are_at_most_75_characters() {
        let mb = Mailbox::new(1, 20_000);
        let mut seen = 0;
        for i in 0..mb.len() {
            let raw = String::from_utf8_lossy(&mb.raw(i)).into_owned();
            for word in raw
                .split_whitespace()
                .filter(|w| w.starts_with("=?UTF-8?B?"))
            {
                assert!(word.len() <= 75 && word.ends_with("?="), "{word}");
                seen += 1;
            }
        }
        assert!(seen > 20, "encoded words: {seen}");
    }

    #[test]
    fn fields_match_the_rendered_text() {
        let mb = Mailbox::new(9, 3000);
        let (mut records, mut asks, mut owner_asks) = (0, 0, 0);
        for i in 0..mb.len() {
            let f: std::collections::BTreeMap<_, _> = mb.fields(i).into_iter().collect();
            let raw = String::from_utf8_lossy(&mb.raw(i)).into_owned();
            let t = mb.truth(i);
            if t.sender == Sender::Hostile {
                assert!(f.is_empty());
            } else if t.form == Form::Record {
                records += 1;
                assert!(raw.contains(&format!("Total: {} EUR", f["fields.amount.value"])));
                assert!(raw.contains(&format!("order {}", f["fields.order"])));
                assert!(raw.contains(&format!("From: \"{}\"", f["fields.vendor"])));
            } else if t.form == Form::Commitment && t.message_id.contains(".0@") {
                asks += 1;
                assert_eq!(f["kind"], "task");
                // The first message asks its recipient to send a document. If
                // the owner asks, the other party owes the owner.
                let want = if t.sender == Sender::Owner {
                    owner_asks += 1;
                    assert!(raw.contains(&format!("From: \"{OWNER_NAME}\" <{OWNER}>")));
                    "owed_to_you"
                } else {
                    assert!(raw.contains(&format!("To: \"{OWNER_NAME}\" <{OWNER}>")));
                    "you_owe"
                };
                assert_eq!(f["direction"], want, "message {i}");
                assert!(f["due.date"].as_str() > date::ymd(t.date).as_str());
            } else {
                assert!(f.is_empty(), "message {i}: {f:?}");
            }
        }
        assert!(
            records > 100 && asks > 50 && owner_asks > 0 && owner_asks < asks,
            "{records} {asks} {owner_asks}"
        );
    }

    #[test]
    fn replies_point_to_the_thread() {
        let mb = Mailbox::new(5, 2000);
        let reply =
            (0..mb.len()).find(|i| mb.truth(*i).message_id.ends_with(".1@synth.example.com>"));
        let i = reply.expect("a reply");
        let raw = String::from_utf8_lossy(&mb.raw(i)).into_owned();
        let parent = mb.truth(i).message_id.replace(".1@", ".0@");
        assert!(raw.contains(&format!("In-Reply-To: {parent}")), "{raw}");
    }
}
