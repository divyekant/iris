//! The IMAP client against the fake IMAP server and against scripted
//! server bytes.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Cursor, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::Duration;

use iris_fakeimap::{Caps, FakeImap, PASSWORD, TOKEN, USER};
use iris_imap::{Auth, Change, Error, Session, Special, RESPONSE_LIMIT};

const PASS: Auth<'static> = Auth::Password {
    user: USER,
    password: PASSWORD,
};

fn open(fake: &FakeImap, auth: Auth<'_>) -> iris_imap::Result<Session<TcpStream>> {
    let s = TcpStream::connect(fake.addr()).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    Session::start(s, auth)
}

/// A small xorshift generator, so that a failure repeats.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

#[test]
fn sign_in_each_way() {
    let fake = FakeImap::start(Caps::ALL);
    let s = open(&fake, PASS).unwrap();
    assert!(s.qresync() && s.condstore());
    s.logout().unwrap();
    open(
        &fake,
        Auth::OAuth {
            user: USER,
            token: TOKEN,
        },
    )
    .unwrap();

    let no_plain = FakeImap::start(Caps {
        plain: false,
        ..Caps::ALL
    });
    open(&no_plain, PASS).unwrap();
    assert!(no_plain.commands().iter().any(|c| c.contains("LOGIN ***")));
    assert!(no_plain.commands().iter().all(|c| !c.contains(PASSWORD)));
}

#[test]
fn bad_password() {
    for caps in [
        Caps::ALL,
        Caps {
            plain: false,
            ..Caps::ALL
        },
    ] {
        let fake = FakeImap::start(caps);
        let r = open(
            &fake,
            Auth::Password {
                user: USER,
                password: "wrong",
            },
        );
        assert!(matches!(r, Err(Error::SignIn)), "{:?}", r.err());
    }
}

#[test]
fn bad_token() {
    // The fake sends the XOAUTH2 error challenge and waits for an empty line.
    let fake = FakeImap::start(Caps::ALL);
    let r = open(
        &fake,
        Auth::OAuth {
            user: USER,
            token: "old",
        },
    );
    assert!(matches!(r, Err(Error::SignIn)), "{:?}", r.err());
}

#[test]
fn folders_and_examine() {
    let fake = FakeImap::start(Caps::ALL);
    fake.create("Sent", Some("\\Sent"));
    fake.create("Odd \"name\"", None);
    let mut s = open(&fake, PASS).unwrap();
    let folders = s.folders().unwrap();
    let names: Vec<_> = folders.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["INBOX", "Odd \"name\"", "Sent"]);
    assert_eq!(folders[2].special, Some(Special::Sent));
    fake.add("Odd \"name\"", b"x".to_vec(), &[]);
    let e = s.examine("Odd \"name\"").unwrap();
    assert_eq!((e.exists, e.uidnext), (1, Some(2)));
    assert!(e.highest_modseq.is_some());
    assert!(fake
        .commands()
        .iter()
        .any(|c| c.starts_with("a") && c.contains("EXAMINE") && c.ends_with("(CONDSTORE)")));
}

#[test]
fn examine_missing() {
    let fake = FakeImap::start(Caps::ALL);
    let mut s = open(&fake, PASS).unwrap();
    assert!(matches!(s.examine("Nope"), Err(Error::Refused(_))));
}

#[test]
fn bad_name() {
    let fake = FakeImap::start(Caps::ALL);
    let mut s = open(&fake, PASS).unwrap();
    let before = fake.commands().len();
    assert!(matches!(s.examine("a\r\nb LOGOUT"), Err(Error::Name)));
    assert_eq!(fake.commands().len(), before, "nothing sent");
}

#[test]
fn no_condstore_mode() {
    let fake = FakeImap::start(Caps {
        condstore: false,
        qresync: false,
        plain: true,
    });
    let mut s = open(&fake, PASS).unwrap();
    assert!(!s.condstore());
    let e = s.examine("INBOX").unwrap();
    assert_eq!(e.highest_modseq, None);
    assert!(matches!(
        s.changes(1, |_| {}),
        Err(Error::Missing("CONDSTORE"))
    ));
}

#[test]
fn list_range() {
    let fake = FakeImap::start(Caps::ALL);
    for _ in 0..3 {
        fake.add("INBOX", b"x".to_vec(), &["\\Seen"]);
    }
    let mut s = open(&fake, PASS).unwrap();
    s.examine("INBOX").unwrap();
    let mut got = Vec::new();
    // The fake, like a real server, gives UID 3 for 10:*.
    s.list(10, None, |e| got.push(e.uid)).unwrap();
    assert!(got.is_empty(), "{got:?}");
    let mut entries = Vec::new();
    s.list(2, Some(3), |e| entries.push(e)).unwrap();
    assert_eq!(entries.iter().map(|e| e.uid).collect::<Vec<_>>(), [2, 3]);
    assert_eq!(
        entries[0].internal_date.as_deref(),
        Some(iris_fakeimap::DATE)
    );
    assert_eq!(
        (entries[0].size, entries[0].flags.clone()),
        (Some(1), vec!["\\Seen".to_owned()])
    );
}

#[test]
fn fetch_raw_streams() {
    let mail = iris_synth::Mailbox::new(7, 3);
    let fake = FakeImap::start(Caps::ALL);
    let uid = fake.add("INBOX", mail.raw(1), &[]);
    // Larger than the response limit: it works only if the client streams it.
    let big: Vec<u8> = (0..RESPONSE_LIMIT * 20)
        .map(|i| b"abcdefgh\r\n"[i % 10])
        .collect();
    let big_uid = fake.add("INBOX", big.clone(), &[]);
    let mut s = open(&fake, PASS).unwrap();
    s.examine("INBOX").unwrap();
    let mut out = Vec::new();
    assert_eq!(s.fetch_raw(uid, &mut out).unwrap(), Some(out.len() as u64));
    assert_eq!(out, mail.raw(1));
    let mut out = Vec::new();
    assert_eq!(
        s.fetch_raw(big_uid, &mut out).unwrap(),
        Some(big.len() as u64)
    );
    assert!(out == big);
    assert_eq!(s.fetch_raw(99, &mut io::sink()).unwrap(), None);
    assert!(fake.commands().iter().any(|c| c.contains("BODY.PEEK[]")));
}

#[test]
fn no_plain() {
    // A server with no TLS: the handshake fails, and nothing falls back to plain.
    let fake = FakeImap::start(Caps::ALL);
    let r = iris_imap::connect("127.0.0.1", fake.addr().port(), Duration::from_secs(5));
    assert!(r.is_err());
    assert!(fake.commands().is_empty());
}

#[test]
fn stall() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    thread::spawn(move || {
        let held = listener.accept();
        thread::sleep(Duration::from_secs(5));
        drop(held);
    });
    let s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_millis(200)))
        .unwrap();
    assert!(matches!(Session::start(s, PASS), Err(Error::Io(_))));
}

/// A stream that reads scripted server bytes and drops what the client
/// writes.
struct Script(Cursor<Vec<u8>>);

impl Read for Script {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        self.0.read(b)
    }
}

impl Write for Script {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        Ok(b.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

const SIGN_IN: &str = "* OK ready\r\n* CAPABILITY IMAP4rev1 AUTH=PLAIN\r\na1 OK done\r\n+ \r\na2 OK in\r\n* CAPABILITY IMAP4rev1 CONDSTORE\r\na3 OK done\r\n";

fn script(rest: &[u8]) -> Script {
    Script(Cursor::new([SIGN_IN.as_bytes(), rest].concat()))
}

#[test]
fn too_large() {
    let long_line = format!("* OK {}\r\n", "x".repeat(RESPONSE_LIMIT));
    let big_literal = format!("* LIST () \"/\" {{{}}}\r\n", RESPONSE_LIMIT + 1);
    for rest in [long_line, big_literal] {
        let mut s = Session::start(script(rest.as_bytes()), PASS).unwrap();
        assert!(matches!(s.folders(), Err(Error::TooLarge)));
    }
}

#[test]
fn huge_literal() {
    let rest = format!("* LIST () \"/\" {{{}}}\r\n", usize::MAX);
    let mut s = Session::start(script(rest.as_bytes()), PASS).unwrap();
    assert!(matches!(s.folders(), Err(Error::TooLarge)));
}

/// A server that echoes the secret in each answer to the sign-in: no error
/// shows it.
#[test]
fn sign_in_errors_hide_the_secret() {
    let secret = "s3cret-Value";
    for (caps, auth) in [("AUTH=PLAIN", 0), ("", 0), ("AUTH=XOAUTH2", 1)] {
        for answer in [
            format!("a2 NO bad {secret}\r\n"),
            format!("a2 BAD bad {secret}\r\n"),
            format!("a2 NO [ALERT] {secret}\r\n"),
            format!("* {secret} (\r\n"),
            format!("a9 OK {secret}\r\n"),
        ] {
            let challenge = if caps.starts_with("AUTH") {
                "+ \r\n"
            } else {
                ""
            };
            let bytes = format!(
                "* OK ready\r\n* CAPABILITY IMAP4rev1 {caps}\r\na1 OK done\r\n{challenge}{answer}"
            );
            let a = match auth {
                0 => Auth::Password {
                    user: USER,
                    password: secret,
                },
                _ => Auth::OAuth {
                    user: USER,
                    token: secret,
                },
            };
            let err = Session::start(Script(Cursor::new(bytes.into_bytes())), a)
                .err()
                .expect("error");
            let shown = format!("{err} {err:?}");
            assert!(!shown.contains(secret), "{shown}");
        }
    }
}

/// A removal of the highest UID, with no later add, shows in VANISHED.
#[test]
fn vanished_highest_uid() {
    let fake = FakeImap::start(Caps::ALL);
    fake.add("INBOX", b"a".to_vec(), &[]);
    let top = fake.add("INBOX", b"b".to_vec(), &[]);
    let mut s = open(&fake, PASS).unwrap();
    let since = s.examine("INBOX").unwrap().highest_modseq.unwrap();
    fake.remove("INBOX", top);
    s.examine("INBOX").unwrap();
    let mut got = Vec::new();
    s.changes(since, |c| got.push(c)).unwrap();
    assert_eq!(got, [Change::Vanished(top..=top)]);
}

/// A server that echoes the secret after the sign-in (to CAPABILITY, to
/// ENABLE, or as a bad response): no error shows it.
#[test]
fn errors_after_sign_in_hide_the_secret() {
    let secret = "s3cret-Value";
    let head =
        "* OK ready\r\n* CAPABILITY IMAP4rev1 AUTH=PLAIN\r\na1 OK done\r\n+ \r\na2 OK in\r\n";
    for rest in [
        format!("a3 NO {secret}\r\n"),
        format!("* {secret} (\r\n"),
        format!("* CAPABILITY IMAP4rev1 QRESYNC\r\na3 OK done\r\na4 BAD {secret}\r\n"),
        format!("* CAPABILITY IMAP4rev1 QRESYNC\r\na3 OK done\r\n* OK [{secret}\r\n"),
    ] {
        let auth = Auth::Password {
            user: USER,
            password: secret,
        };
        let err = Session::start(
            Script(Cursor::new(format!("{head}{rest}").into_bytes())),
            auth,
        )
        .err()
        .expect("error");
        let shown = format!("{err} {err:?}");
        assert!(!shown.contains(secret), "{shown}");
    }
}

#[test]
fn other_uid() {
    let rest = b"* 1 FETCH (UID 9 BODY[] {3}\r\nabc)\r\na4 OK done\r\n";
    let mut s = Session::start(script(rest), PASS).unwrap();
    assert!(matches!(
        s.fetch_raw(5, &mut io::sink()),
        Err(Error::Protocol(_))
    ));
}

#[test]
fn endless_challenge() {
    let s = Script(Cursor::new(b"* OK ready\r\n* CAPABILITY IMAP4rev1 AUTH=PLAIN\r\na1 OK done\r\n+ \r\n+ x\r\n+ x\r\n+ x\r\n".to_vec()));
    assert!(matches!(Session::start(s, PASS), Err(Error::Protocol(_))));
}

#[test]
fn rev2_only() {
    let s = Script(Cursor::new(
        b"* OK ready\r\n* CAPABILITY IMAP4rev2 AUTH=PLAIN\r\na1 OK done\r\n".to_vec(),
    ));
    // The parser refuses a CAPABILITY list without IMAP4rev1.
    assert!(matches!(Session::start(s, PASS), Err(Error::Protocol(_))));
}

#[test]
fn bye_closes() {
    let mut s = Session::start(script(b"* BYE going away\r\n"), PASS).unwrap();
    assert!(matches!(s.folders(), Err(Error::Closed)));
}

/// Random changes to the bytes of a valid session never make the client
/// panic or hang.
#[test]
fn fuzz_responses() {
    let valid = [
        SIGN_IN.as_bytes(),
        b"* LIST (\\HasNoChildren \\Sent) \"/\" \"Sent\"\r\n* LIST () \"/\" {5}\r\nIN\"BX\r\na4 OK done\r\n",
        b"* 3 EXISTS\r\n* OK [UIDVALIDITY 7] x\r\n* OK [UIDNEXT 4] x\r\n* OK [HIGHESTMODSEQ 90] x\r\na5 OK [READ-ONLY] done\r\n",
        b"* VANISHED (EARLIER) 1:2,5\r\n* 1 FETCH (UID 3 FLAGS (\\Seen $x) MODSEQ (91))\r\na6 OK done\r\n",
        b"* 1 FETCH (UID 3 BODY[] {4}\r\nab\r\n)\r\na7 OK done\r\n",
    ]
    .concat();
    let mut rng = Rng(0x9e3779b97f4a7c15);
    for round in 0..20_000 {
        let mut bytes = valid.clone();
        for _ in 0..1 + rng.below(4) {
            if bytes.is_empty() {
                break;
            }
            let at = rng.below(bytes.len() as u64) as usize;
            match rng.below(4) {
                0 => bytes[at] = rng.next() as u8,
                1 => {
                    bytes.remove(at);
                }
                2 => bytes.insert(at, b"{}()[]\r\n 0123456789*\"\\"[rng.below(22) as usize]),
                _ => bytes.truncate(at),
            }
        }
        let run = || -> iris_imap::Result<()> {
            let mut s = Session::start(Script(Cursor::new(bytes.clone())), PASS)?;
            s.folders()?;
            s.examine("INBOX")?;
            s.changes(90, |_| {})?;
            s.fetch_raw(3, &mut io::sink())?;
            Ok(())
        };
        let _ = std::panic::catch_unwind(run).unwrap_or_else(|_| panic!("panic in round {round}"));
    }
}

/// The mirror that part B keeps: (UIDVALIDITY, mod-sequence, UID to flags).
#[derive(Default)]
struct Mirror {
    validity: u32,
    modseq: Option<u64>,
    map: BTreeMap<u32, Vec<String>>,
}

fn sync(s: &mut Session<TcpStream>, m: &mut Mirror) {
    let e = s.examine("INBOX").unwrap();
    if e.uidvalidity != m.validity {
        *m = Mirror {
            validity: e.uidvalidity,
            ..Mirror::default()
        };
    }
    match m.modseq.filter(|_| s.condstore()) {
        Some(since) => {
            let mut gone = Vec::new();
            s.changes(since, |c| match c {
                Change::Entry(e) => {
                    m.map.insert(e.uid, e.flags);
                }
                Change::Vanished(r) => gone.push(r),
            })
            .unwrap();
            for r in gone {
                m.map.retain(|u, _| !r.contains(u));
            }
            if !s.qresync() {
                let mut all = BTreeSet::new();
                s.list(1, None, |e| {
                    all.insert(e.uid);
                })
                .unwrap();
                m.map.retain(|u, _| all.contains(u));
            }
        }
        None => {
            m.map.clear();
            s.list(1, None, |e| {
                m.map.insert(e.uid, e.flags);
            })
            .unwrap();
        }
    }
    m.modseq = e.highest_modseq;
}

/// After random adds, flag changes, removals and new UIDVALIDITY values, the
/// mirror agrees with the server, in each of the three modes.
#[test]
fn mirror_agrees_in_each_mode() {
    let mail = iris_synth::Mailbox::new(3, 40);
    let flag_sets: [&[&str]; 4] = [&[], &["\\Seen"], &["\\Seen", "\\Flagged"], &["$Label1"]];
    for (mode, caps) in [
        Caps::ALL,
        Caps {
            qresync: false,
            ..Caps::ALL
        },
        Caps {
            condstore: false,
            qresync: false,
            plain: true,
        },
    ]
    .into_iter()
    .enumerate()
    {
        let fake = FakeImap::start(caps);
        let mut rng = Rng(42 + mode as u64);
        let mut s = open(&fake, PASS).unwrap();
        let mut mirror = Mirror::default();
        for round in 0..40 {
            for _ in 0..rng.below(6) {
                let uids: Vec<u32> = fake.snapshot("INBOX").1.keys().copied().collect();
                let pick = |rng: &mut Rng| uids[rng.below(uids.len() as u64) as usize];
                match rng.below(10) {
                    0..=3 => {
                        let i = rng.below(mail.len() as u64) as usize;
                        fake.add("INBOX", mail.raw(i), flag_sets[rng.below(4) as usize]);
                    }
                    4..=6 if !uids.is_empty() => {
                        fake.set_flags("INBOX", pick(&mut rng), flag_sets[rng.below(4) as usize])
                    }
                    7..=8 if !uids.is_empty() => fake.remove("INBOX", pick(&mut rng)),
                    9 if rng.below(4) == 0 => fake.renew("INBOX"),
                    _ => {}
                }
            }
            sync(&mut s, &mut mirror);
            let (validity, truth) = fake.snapshot("INBOX");
            assert_eq!(mirror.validity, validity, "mode {mode} round {round}");
            assert_eq!(mirror.map, truth, "mode {mode} round {round}");
        }
        let used: Vec<String> = fake.commands();
        let changed = used.iter().any(|c| c.contains("CHANGEDSINCE"));
        assert_eq!(changed, mode < 2, "mode {mode}");
        assert_eq!(
            used.iter().any(|c| c.contains("VANISHED")),
            mode == 0,
            "mode {mode}"
        );
    }
}
