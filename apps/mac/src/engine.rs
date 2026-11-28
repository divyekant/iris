//! The engine side of the Mac app: the paths, one request on the engine
//! socket, and the state of the notices. Standard library and `serde_json`
//! only, so CI tests this file on Linux.

// On Linux only the tests use this file.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

/// The time between two `app.notices` calls. The engine waits 30 seconds
/// before it shows the notices itself again.
pub const POLL: Duration = Duration::from_secs(10);
/// The most notices of one answer of the engine (`APP_NOTICES_MAX`).
pub const PAGE: usize = 20;
/// The longest wait for the answer of the engine.
const WAIT: Duration = Duration::from_secs(5);
/// The longest answer line.
const MAX_ANSWER: u64 = 1 << 20;

/// The address of the "Start Iris" button of the local page.
pub const START_URL: &str = "https://start.iris.invalid/";
/// The longest run of a fallback `osascript`, as in the engine.
pub const SHOW_WAIT: Duration = Duration::from_secs(5);

/// The wait from now to the next poll. The polls start each [`POLL`] from
/// the start of the last poll, so a slow answer does not move the next one.
pub fn next_poll(since_start: Duration) -> Duration {
    POLL.saturating_sub(since_start)
}

/// True only for the "Start Iris" button of a local page (`tauri:` scheme).
/// A link on the engine page, or another path, starts nothing.
pub fn start_allowed(from_scheme: &str, to: &str) -> bool {
    from_scheme == "tauri" && to == START_URL
}

/// Runs `c` and waits at most `wait`. Kills it after that, so a stalled
/// command does not hold the thread.
pub fn run(mut c: Command, wait: Duration) -> Result<(), String> {
    let mut child = c.spawn().map_err(|e| e.to_string())?;
    let end = Instant::now() + wait;
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return if status.success() {
                Ok(())
            } else {
                Err(format!("the command ended with {status}"))
            };
        }
        if Instant::now() >= end {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("no answer in {} ms", wait.as_millis()));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The engine socket in the default data folder of macOS (issue #541).
pub fn socket(home: &Path) -> PathBuf {
    home.join("Library/Application Support/iris/iris.sock")
}

/// The `iris` program. A Mac app gets no `PATH` of a shell, so the app looks
/// at the path of `scripts/install.sh` first, then at the paths of Homebrew.
pub fn program(home: &Path, exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    [
        home.join(".local/bin/iris"),
        PathBuf::from("/opt/homebrew/bin/iris"),
        PathBuf::from("/usr/local/bin/iris"),
    ]
    .into_iter()
    .find(|p| exists(p))
}

/// True when an engine listens on the socket at `path`. It sends no verb.
pub fn listening(path: &Path) -> bool {
    UnixStream::connect(path).is_ok()
}

/// Send one request to the engine socket at `path`, and give the result.
pub fn request(path: &Path, verb: &str, args: Value) -> Result<Value, String> {
    let mut s = UnixStream::connect(path).map_err(|e| format!("no engine: {e}"))?;
    s.set_read_timeout(Some(WAIT)).map_err(|e| e.to_string())?;
    s.set_write_timeout(Some(WAIT)).map_err(|e| e.to_string())?;
    let mut line = json!({ "v": 1, "verb": verb, "args": args }).to_string();
    line.push('\n');
    s.write_all(line.as_bytes()).map_err(|e| e.to_string())?;
    let mut answer = String::new();
    BufReader::new((&s).take(MAX_ANSWER))
        .read_line(&mut answer)
        .map_err(|e| e.to_string())?;
    let a: Value = serde_json::from_str(&answer).map_err(|_| "not an answer".to_string())?;
    match a["ok"].as_bool() {
        Some(true) => Ok(a["result"].clone()),
        Some(false) => Err(a["error"].as_str().unwrap_or("refused").to_string()),
        None => Err("not an answer".into()),
    }
}

/// The sign-in address of the engine, only when it is a loopback address
/// with a code: `http://127.0.0.1:<port>/#code=<hex>` or the same on `[::1]`.
/// The web view loads no other address from the socket.
pub fn checked_url(url: &str) -> Option<&str> {
    let rest = url
        .strip_prefix("http://127.0.0.1:")
        .or_else(|| url.strip_prefix("http://[::1]:"))?;
    let (port, code) = rest.split_once("/#code=")?;
    // A port of 1 to 65535 with no sign and no leading zero.
    let port_ok = port.bytes().all(|b| b.is_ascii_digit())
        && !port.starts_with('0')
        && port.parse::<u16>().is_ok();
    let code_ok =
        !code.is_empty() && code.len() <= 64 && code.bytes().all(|b| b.is_ascii_hexdigit());
    (port_ok && code_ok).then_some(url)
}

/// How long the route of a notice click counts as the view of the window.
/// The click changes the hash of the page with a script that runs some
/// milliseconds later, so the address of the window can still show the old
/// view in this time.
pub const CLICK_HOLD: Duration = Duration::from_secs(5);

/// The view that a new sign-in of the window keeps (issue #603). `click` is
/// the route of the newest notice click and its age. `fragment` is the
/// fragment of the window address now, from the parsed URL. The click wins
/// while it is younger than [`CLICK_HOLD`]; else the fragment counts, but
/// only a route of the web client (`/...`), never a sign-in code.
pub fn kept_view(click: Option<(&str, Duration)>, fragment: Option<&str>) -> Option<String> {
    if let Some((route, age)) = click {
        if age < CLICK_HOLD {
            return Some(route.to_string());
        }
    }
    fragment
        .filter(|f| f.starts_with('/'))
        .map(|f| format!("#{f}"))
}

/// One notice to show.
#[derive(Debug, PartialEq)]
pub struct Notice {
    pub title: String,
    pub body: String,
    /// The route of the web client that a click opens.
    pub route: String,
}

/// One answer of `app.notices`.
#[derive(Debug, PartialEq)]
pub struct Page {
    pub notices: Vec<Notice>,
    /// The count of notices that are not done.
    pub open: u64,
    /// True when the page was full: ask again at once.
    pub more: bool,
    /// True when the engine started again since the last answer: the open
    /// window needs a new sign-in (issue #547).
    pub restarted: bool,
}

/// True when the open window needs a new sign-in address: the engine
/// started again (`restarted`), or the window shows a local page while the
/// engine answers (`signed_in` is false), for example when the app opened
/// before the engine.
pub fn sign_in_needed(restarted: bool, signed_in: bool) -> bool {
    restarted || !signed_in
}

/// The state of the notices: the largest notice row ID that the app read.
#[derive(Debug, Default)]
pub struct Poll {
    last: Option<u64>,
    /// The start of the engine in the last answer.
    engine: Option<u64>,
}

impl Poll {
    /// The args of the next call. The first call has no `after`: the engine
    /// gives only `last`, so the app shows no old notice.
    pub fn args(&self) -> Value {
        match self.last {
            None => json!({}),
            Some(n) => json!({ "after": n }),
        }
    }

    /// Read one result. `None` when it is not valid: the state stays, and the
    /// next call asks again. So the cursor never passes a notice that the
    /// app did not get, unless the engine shows it. A valid result is one
    /// of these:
    /// - `start`: a new start of the app (the first call, or a call after a
    ///   gap). No notices, any `last`: the engine shows the notices up to it;
    /// - after the first call, no notices and `last` equal to the cursor;
    /// - after the first call, 1 to [`PAGE`] notices with row IDs above the
    ///   cursor, in increasing order, and `last` equal to the largest ID.
    pub fn read(&mut self, r: &Value) -> Option<Page> {
        let last = r["last"].as_u64()?;
        let open = r["open"].as_u64()?;
        let engine = r["engine"].as_u64()?;
        let start = r["start"].as_bool()?;
        let list = r["notices"].as_array()?;
        if start {
            if !list.is_empty() {
                return None;
            }
        } else if list.is_empty() {
            if self.last != Some(last) {
                return None;
            }
        } else {
            let ids = list
                .iter()
                .map(|n| n["id"].as_str()?.strip_prefix("n-")?.parse::<u64>().ok())
                .collect::<Option<Vec<_>>>()?;
            let after = self.last?;
            let rising = ids.windows(2).all(|w| w[0] < w[1]);
            let ok = ids.len() <= PAGE && rising && ids[0] > after && ids.last() == Some(&last);
            if !ok {
                return None;
            }
        }
        let notices = list.iter().map(notice).collect::<Option<Vec<_>>>()?;
        self.last = Some(last);
        let restarted = self.engine.is_some_and(|e| e != engine);
        self.engine = Some(engine);
        Some(Page {
            more: notices.len() >= PAGE,
            notices,
            open,
            restarted,
        })
    }
}

fn notice(v: &Value) -> Option<Notice> {
    let s = |k: &str| v[k].as_str();
    // An ID goes into the route only when it is a plain ID (`m-42`, `j-7`).
    // A job notice opens its job (`docs/specs/2026-11-25-job-notices.md`).
    let route = match (v["job"].as_str(), v["message"]["id"].as_str()) {
        (Some(id), _) if id.starts_with("j-") && plain_id(id) => format!("#/jobs/{id}"),
        (_, Some(id)) if plain_id(id) => format!("#/mail/{id}"),
        _ => "#/brief".to_string(),
    };
    // The engine cuts the text of a job notice for the system notice; the
    // body of a rule notice stays whole (`docs/specs/2026-11-16-notices.md`).
    Some(Notice {
        title: s("rule_name")?.to_string(),
        body: format!("{}: {}", s("title")?, s("text")?),
        route,
    })
}

fn plain_id(id: &str) -> bool {
    let Some((kind, n)) = id.split_once('-') else {
        return false;
    };
    kind.len() == 1
        && kind.bytes().all(|b| b.is_ascii_lowercase())
        && !n.is_empty()
        && n.bytes().all(|b| b.is_ascii_digit())
}

/// The system notice when the notice API of the app fails: the fixed script
/// of the engine (`crates/iris/src/notify.rs`). The words are arguments,
/// never part of the script.
pub fn osascript(title: &str, body: &str) -> Command {
    let mut c = Command::new("/usr/bin/osascript");
    c.args([
        "-e",
        "on run argv",
        "-e",
        "display notification (item 2 of argv) with title (item 1 of argv)",
        "-e",
        "end run",
    ]);
    // `osascript` reads a first word that starts with `-` as an option.
    c.args([format!("Iris: {title}"), body.to_string()])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_sign_in_keeps_the_newest_click_or_the_route_of_the_page() {
        let seed = std::cell::Cell::new(7u64);
        let next = |n: u64| {
            seed.set(
                seed.get()
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407),
            );
            (seed.get() >> 33) % n
        };
        let pieces = ["/", "mail/m-4", "code=ab12", "brief", "#", "", "/x?y"];
        for _ in 0..5_000 {
            let fragment: Option<String> = (next(4) > 0).then(|| {
                (0..next(3))
                    .map(|_| pieces[next(pieces.len() as u64) as usize])
                    .collect()
            });
            let age = Duration::from_millis(next(10_000));
            let click = (next(2) == 0).then_some(("#/mail/m-9", age));
            let got = kept_view(click, fragment.as_deref());
            let want = match (click, fragment.as_deref()) {
                (Some((r, a)), _) if a < CLICK_HOLD => Some(r.to_string()),
                (_, Some(f)) if f.starts_with('/') => Some(format!("#{f}")),
                _ => None,
            };
            assert_eq!(got, want, "{click:?} {fragment:?}");
            // Never a sign-in code, and always a route of the web client.
            assert!(got.as_deref().is_none_or(|g| g.starts_with("#/")));
        }
        // The order of issue #603: a click, then the new sign-in before the
        // hash change of the click ran in the page.
        assert_eq!(
            kept_view(Some(("#/mail/m-9", Duration::ZERO)), Some("/brief")).as_deref(),
            Some("#/mail/m-9")
        );
        // An old click does not win over where the person went since.
        assert_eq!(
            kept_view(Some(("#/mail/m-9", CLICK_HOLD)), Some("/brief")).as_deref(),
            Some("#/brief")
        );
        // The wiring in mac.rs: each route of `show` is a click, and the new
        // sign-in reads the view before the navigate.
        let mac = include_str!("mac.rs");
        let show = &mac[mac.find("fn show(").unwrap()..];
        let show = &show[..show.find("\n}\n").unwrap()];
        assert!(show.contains(".click.lock()"), "{show}");
        let again = &mac[mac.find("fn sign_in_again").unwrap()..];
        let again = &again[..again.find("\n}\n").unwrap()];
        let read = again.find("engine::kept_view(").unwrap();
        let navigate = again.find(".navigate(").unwrap();
        assert!(read < navigate, "{again}");
        assert!(again.contains(".fragment()"), "{again}");
        assert!(again.contains(".route.lock()"), "{again}");
    }

    #[test]
    fn a_stalled_command_ends_at_the_time_limit() {
        let mut c = Command::new("sleep");
        c.arg("30");
        let t = Instant::now();
        assert!(run(c, Duration::from_millis(200)).is_err());
        assert!(t.elapsed() < Duration::from_secs(2));
        assert!(run(Command::new("true"), SHOW_WAIT).is_ok());
        assert!(run(Command::new("false"), SHOW_WAIT).is_err());
    }

    #[test]
    fn the_notice_thread_never_waits_for_osascript_without_a_limit() {
        let mac = include_str!("mac.rs");
        let all = mac.matches("engine::osascript(").count();
        let limited = mac.matches("engine::run(engine::osascript(").count();
        assert!(all > 0);
        assert_eq!(all, limited);
        assert!(mac.contains("engine::next_poll("));
    }

    #[test]
    fn each_poll_starts_at_most_one_poll_time_after_the_last() {
        // A restart of the engine is seen at the next poll start. With a
        // socket wait of up to WAIT, the next start stays POLL after the last.
        let mut e = Duration::ZERO;
        while e <= Duration::from_secs(30) {
            let gap = e + next_poll(e);
            assert_eq!(gap, POLL.max(e));
            e += Duration::from_millis(250);
        }
        assert_eq!(next_poll(WAIT), POLL - WAIT);
    }

    #[test]
    fn only_the_button_of_the_local_page_starts_the_engine() {
        assert!(start_allowed("tauri", START_URL));
        for (from, to) in [
            ("http", START_URL),
            ("https", START_URL),
            ("tauri", "https://start.iris.invalid/x"),
            ("tauri", "https://start.iris.invalid/?a"),
            ("tauri", "http://start.iris.invalid/"),
            ("tauri", "https://start.iris.invalid.example/"),
        ] {
            assert!(!start_allowed(from, to), "{from} {to}");
        }
        assert!(include_str!("mac.rs").contains("engine::start_allowed("));
    }

    #[test]
    fn only_a_loopback_sign_in_address_loads() {
        for ok in [
            "http://127.0.0.1:4800/#code=0a1b2c",
            "http://[::1]:4800/#code=ff",
        ] {
            assert_eq!(checked_url(ok), Some(ok));
        }
        for bad in [
            "http://127.0.0.1.evil.example:4800/#code=00",
            "https://127.0.0.1:4800/#code=00",
            "http://localhost:4800/#code=00",
            "http://127.0.0.1:4800/#code=",
            "http://127.0.0.1:4800/#code=zz",
            "http://127.0.0.1:/#code=00",
            "http://127.0.0.1:4800/x#code=00",
            "http://127.0.0.1:4800/#code=00&x=1",
            "javascript:alert(1)",
            "",
        ] {
            assert_eq!(checked_url(bad), None, "{bad}");
        }
    }

    /// A small deterministic random source for the property tests.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn pick<'a>(&mut self, from: &[&'a str]) -> &'a str {
            from[(self.next() % from.len() as u64) as usize]
        }
    }

    /// The grammar of a sign-in address, written apart from `checked_url`.
    fn in_grammar(url: &str) -> bool {
        let Some(rest) = ["http://127.0.0.1:", "http://[::1]:"]
            .iter()
            .find_map(|p| url.strip_prefix(p))
        else {
            return false;
        };
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        let (port, tail) = rest.split_at(digits);
        let port_ok = matches!(port.parse::<u32>(), Ok(1..=65535)) && !port.starts_with('0');
        let code = tail.strip_prefix("/#code=").unwrap_or("");
        port_ok && (1..=64).contains(&code.len()) && code.bytes().all(|b| b.is_ascii_hexdigit())
    }

    #[test]
    fn each_accepted_url_is_a_loopback_sign_in_address() {
        let parts = [
            "http://",
            "https://",
            "127.0.0.1",
            "[::1]",
            "localhost",
            "example.com",
            ":",
            "4800",
            "0",
            "65535",
            "65536",
            "99999",
            "/",
            "#code=",
            "/#code=",
            "ab12",
            "ZZ",
            "%2F",
            "@",
            "?",
            "&",
            "\"",
            "<",
            " ",
            "\n",
            "javascript:",
            "..",
            "x",
        ];
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
        for _ in 0..50_000 {
            let n = 1 + rng.next() % 8;
            let url: String = (0..n).map(|_| rng.pick(&parts)).collect();
            assert_eq!(checked_url(&url).is_some(), in_grammar(&url), "{url}");
        }
        // Each valid address is accepted, and one changed byte of it is
        // accepted only when the result is still in the grammar.
        let hostile = b"\"<> /@#%g0\n";
        for _ in 0..20_000 {
            let host = rng.pick(&["127.0.0.1", "[::1]"]);
            let port = 1 + rng.next() % 65535;
            let len = 1 + (rng.next() % 64) as usize;
            let code: String = (0..len)
                .map(|_| char::from(b"0123456789abcdefABCDEF"[(rng.next() % 22) as usize]))
                .collect();
            let url = format!("http://{host}:{port}/#code={code}");
            assert_eq!(checked_url(&url), Some(url.as_str()));
            let mut bytes = url.clone().into_bytes();
            let at = (rng.next() % bytes.len() as u64) as usize;
            bytes[at] = hostile[(rng.next() % hostile.len() as u64) as usize];
            let changed = String::from_utf8(bytes).unwrap();
            assert_eq!(
                checked_url(&changed).is_some(),
                in_grammar(&changed),
                "{changed}"
            );
        }
    }

    #[test]
    fn the_program_comes_from_the_install_path_first() {
        let home = Path::new("/Users/p");
        let all = |_: &Path| true;
        assert_eq!(program(home, all), Some(home.join(".local/bin/iris")));
        let brew = |p: &Path| p.starts_with("/opt/homebrew") || p.starts_with("/usr/local");
        assert_eq!(
            program(home, brew),
            Some(PathBuf::from("/opt/homebrew/bin/iris"))
        );
        assert_eq!(program(home, |_| false), None);
        assert_eq!(
            socket(home),
            PathBuf::from("/Users/p/Library/Application Support/iris/iris.sock")
        );
    }

    fn page(ids: &[u64], last: u64) -> Value {
        let notices: Vec<Value> = ids
            .iter()
            .map(|i| {
                json!({ "id": format!("n-{i}"), "rule_name": "Clinic", "title": "Dr Lee",
                        "text": "Results", "message": { "id": format!("m-{i}") } })
            })
            .collect();
        json!({ "notices": notices, "last": last, "open": ids.len(), "engine": 5, "start": false })
    }

    #[test]
    fn the_poll_asks_after_the_last_notice_and_reads_pages() {
        let mut p = Poll::default();
        assert_eq!(p.args(), json!({}));
        // The first answer: no notice, only the last ID.
        let first = p
            .read(&json!({ "notices": [], "last": 7, "open": 2, "engine": 5, "start": true }))
            .unwrap();
        assert_eq!(
            (first.notices.len(), first.open, first.more, first.restarted),
            (0, 2, false, false)
        );
        assert_eq!(p.args(), json!({ "after": 7 }));
        let ids: Vec<u64> = (8..8 + PAGE as u64).collect();
        let full = p.read(&page(&ids, 27)).unwrap();
        assert!(full.more && !full.restarted);
        assert_eq!(
            full.notices[0],
            Notice {
                title: "Clinic".into(),
                body: "Dr Lee: Results".into(),
                route: "#/mail/m-8".into()
            }
        );
        assert_eq!(p.args(), json!({ "after": 27 }));
        // A new engine start: the window signs in again.
        let again = p
            .read(&json!({ "notices": [], "last": 27, "open": 0, "engine": 6, "start": false }))
            .unwrap();
        assert!(again.restarted);
        assert!(sign_in_needed(again.restarted, true));
        // The app opened before the engine: the first answer signs in.
        assert!(sign_in_needed(first.restarted, false));
        assert!(!sign_in_needed(full.restarted, true));
        // A result that is not valid changes nothing.
        for bad in [
            json!(null),
            json!({ "notices": [], "open": 1 }),
            json!({ "notices": [{}], "last": 30, "open": 1 }),
        ] {
            assert_eq!(p.read(&bad), None);
            assert_eq!(p.args(), json!({ "after": 27 }));
        }
    }

    #[test]
    fn the_cursor_moves_only_with_a_valid_page() {
        let mut rng = Rng(0x2545_f491_4f6c_dd1d);
        for _ in 0..30_000 {
            let mut p = Poll::default();
            let first = rng.next().is_multiple_of(3);
            let after = rng.next() % 50;
            if !first {
                let r =
                    json!({ "notices": [], "last": after, "open": 0, "engine": 5, "start": true });
                p.read(&r).unwrap();
            }
            // Random IDs around the cursor, in random order, a random `last`
            // and a random `start`: valid and not valid pages.
            let n = (rng.next() % (PAGE as u64 + 3)) as usize;
            let ids: Vec<u64> = (0..n).map(|_| rng.next() % 80).collect();
            let last = match rng.next() % 3 {
                0 => ids.iter().copied().max().unwrap_or(after),
                1 => rng.next() % 1000,
                _ => after,
            };
            let start = rng.next().is_multiple_of(4);
            let mut answer = page(&ids, last);
            answer["start"] = json!(start);
            let before = p.args();
            let valid = if start {
                ids.is_empty()
            } else if first {
                false
            } else if ids.is_empty() {
                last == after
            } else {
                ids.len() <= PAGE
                    && ids.windows(2).all(|w| w[0] < w[1])
                    && ids[0] > after
                    && ids.last() == Some(&last)
            };
            let read = p.read(&answer);
            let case = format!("first {first} start {start} after {after} {ids:?} last {last}");
            assert_eq!(read.is_some(), valid, "{case}");
            if valid {
                assert_eq!(p.args(), json!({ "after": last }), "{case}");
                assert_eq!(read.unwrap().notices.len(), ids.len());
            } else {
                assert_eq!(
                    p.args(),
                    before,
                    "a page that is not valid moves nothing: {case}"
                );
            }
        }
        // The case of the review: an empty page with no start and a `last`
        // above the cursor would skip notices 8 to 100.
        let mut p = Poll::default();
        p.read(&json!({ "notices": [], "last": 7, "open": 0, "engine": 5, "start": true }))
            .unwrap();
        assert_eq!(p.read(&page(&[], 100)), None);
        assert_eq!(p.args(), json!({ "after": 7 }));
    }

    #[test]
    fn a_click_opens_the_mail_or_the_brief() {
        let n = |message: Value| {
            notice(&json!({ "rule_name": "r", "title": "t", "text": "x", "message": message }))
                .unwrap()
                .route
        };
        assert_eq!(n(json!({ "id": "m-42" })), "#/mail/m-42");
        assert_eq!(n(json!(null)), "#/brief");
        for odd in ["m-", "-1", "mm-1", "m-1/../x", "m-1#x", "M-1", "m-1 "] {
            assert_eq!(n(json!({ "id": odd })), "#/brief", "{odd}");
        }
    }

    #[test]
    fn a_click_on_a_job_notice_opens_the_job() {
        let job = |id: Value| {
            notice(
                &json!({ "rule_name": "Job due", "title": "j-7", "text": "x".repeat(500),
                "message": null, "job": id }),
            )
            .unwrap()
        };
        assert_eq!(job(json!("j-7")).route, "#/jobs/j-7");
        // A rule notice keeps its whole body.
        let long = "s".repeat(200);
        let rule = notice(&json!({ "rule_name": "r", "title": "Ann", "text": long,
            "message": null, "job": null }))
        .unwrap();
        assert_eq!(rule.body, format!("Ann: {long}"));
        for odd in ["j-", "j-7/../x", "m-7", "J-7"] {
            assert_eq!(job(json!(odd)).route, "#/brief", "{odd}");
        }
    }

    #[test]
    fn the_fallback_notice_passes_words_as_arguments() {
        let evil = r#"" & do shell script "rm -rf ~" & "#;
        let c = osascript(evil, evil);
        let a: Vec<String> = c
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(c.get_program(), "/usr/bin/osascript");
        assert_eq!(a[a.len() - 2], format!("Iris: {evil}"));
        assert_eq!(a[a.len() - 1], evil);
        assert!(a[..a.len() - 2].iter().all(|x| !x.contains("rm -rf")));
    }

    #[test]
    fn only_the_notice_thread_calls_app_notices() {
        // A call with no `after` starts the app in the engine. A second
        // caller would take a `last` that the cursor never sees, and the
        // notices after it would get no system notice.
        let mac = include_str!("mac.rs");
        assert_eq!(mac.matches("\"app.notices\"").count(), 1);
        assert!(mac.contains("engine::request(&socket, \"app.notices\", poll.args())"));
    }

    #[test]
    fn a_request_with_no_engine_or_a_bad_answer_is_an_error() {
        let d = std::env::temp_dir().join(format!("iris-mac-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let path = d.join("iris.sock");
        assert!(request(&path, "app.notices", json!({})).is_err());
        assert!(!listening(&path));
        let l = std::os::unix::net::UnixListener::bind(&path).unwrap();
        assert!(listening(&path));
        // The check of `listening` is the first connection: take it.
        drop(l.accept().unwrap());
        let serve = move |answer: &'static str| {
            let (mut s, _) = l.accept().unwrap();
            let mut line = String::new();
            BufReader::new(&s).read_line(&mut line).unwrap();
            let v: Value = serde_json::from_str(&line).unwrap();
            assert_eq!(
                (v["v"].clone(), v["verb"].clone()),
                (json!(1), json!("sign-in"))
            );
            s.write_all(answer.as_bytes()).unwrap();
        };
        let t = std::thread::spawn(move || {
            serve("{\"ok\":true,\"result\":{\"url\":\"u\"}}\n");
            serve("{\"ok\":false,\"code\":3,\"error\":\"no\"}\n");
            serve("not json\n");
        });
        assert_eq!(
            request(&path, "sign-in", json!({})),
            Ok(json!({ "url": "u" }))
        );
        assert_eq!(request(&path, "sign-in", json!({})), Err("no".into()));
        assert!(request(&path, "sign-in", json!({})).is_err());
        t.join().unwrap();
    }
}
