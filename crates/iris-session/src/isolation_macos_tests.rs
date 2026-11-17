//! Tests of the macOS sandbox logic with a fake [`Sys`]. They run on each
//! system. The real process facts run in the Mac acceptance.

use super::*;
use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

/// A process of the fake system.
#[derive(Clone, Copy, Debug)]
struct P {
    coalition: u64,
    stopped: bool,
    unique: u64,
    /// A running process with `fork` makes one child at each scan.
    fork: bool,
}

#[derive(Default)]
struct World {
    boot: String,
    procs: BTreeMap<i32, P>,
    /// (started, exited) of each coalition. A coalition that is not here is
    /// gone.
    counts: BTreeMap<u64, (u64, u64)>,
    counts_fail: bool,
    signals: Vec<(i32, i32)>,
    bootouts: Vec<String>,
    /// A `SIGSTOP` to one of these process IDs finds a new process of the
    /// given coalition there (the ID went to another process).
    reuse_on_stop: BTreeMap<i32, u64>,
    /// `SIGCONT` to these process IDs fails.
    cont_fails: BTreeSet<i32>,
    /// A `SIGCONT` leaves these process IDs stopped.
    stay_stopped: BTreeSet<i32>,
    /// A read of these process IDs fails.
    read_fails: BTreeSet<i32>,
    /// After a `SIGSTOP` to one of these, a read of it fails.
    fail_after_stop: BTreeSet<i32>,
    /// The exit code of each program for `run`, or an error if absent.
    exits: BTreeMap<String, i32>,
    next_pid: i32,
    next_unique: u64,
}

#[derive(Clone, Default)]
struct Fake(Arc<Mutex<World>>);

impl Fake {
    fn w(&self) -> std::sync::MutexGuard<'_, World> {
        self.0.lock().unwrap()
    }

    fn add(&self, coalition: u64, fork: bool) -> i32 {
        let mut w = self.w();
        w.next_pid += 1;
        w.next_unique += 1;
        let (pid, unique) = (w.next_pid + 100, w.next_unique);
        w.procs.insert(
            pid,
            P {
                coalition,
                stopped: false,
                unique,
                fork,
            },
        );
        w.counts.entry(coalition).or_default().0 += 1;
        pid
    }

    fn live(&self, cid: u64) -> usize {
        self.w()
            .procs
            .values()
            .filter(|p| p.coalition == cid)
            .count()
    }
}

impl Sys for Fake {
    fn boot_time(&self) -> io::Result<String> {
        Ok(self.w().boot.clone())
    }

    fn pids(&self) -> io::Result<Vec<i32>> {
        // Each running forker makes a child of its coalition.
        let forkers: Vec<u64> = self
            .w()
            .procs
            .values()
            .filter(|p| p.fork && !p.stopped)
            .map(|p| p.coalition)
            .collect();
        for c in forkers {
            self.add(c, true);
        }
        Ok(self.w().procs.keys().copied().collect())
    }

    fn proc(&self, pid: i32) -> io::Result<Option<Proc>> {
        if pid == std::process::id() as i32 {
            return Ok(Some(Proc {
                coalition: 1,
                stopped: false,
                unique: 1,
            }));
        }
        let w = self.w();
        if w.read_fails.contains(&pid) {
            return Err(io::Error::other("fake"));
        }
        Ok(w.procs.get(&pid).map(|p| Proc {
            coalition: p.coalition,
            stopped: p.stopped,
            unique: p.unique,
        }))
    }

    fn counts(&self, cid: u64) -> io::Result<Option<(u64, u64)>> {
        let w = self.w();
        if w.counts_fail {
            return Err(io::Error::other("fake"));
        }
        Ok(w.counts.get(&cid).copied())
    }

    fn signal(&self, pid: i32, sig: i32) -> io::Result<()> {
        assert!(pid > 0, "a signal to a group: {pid}");
        let mut w = self.w();
        w.signals.push((pid, sig));
        if sig == libc::SIGCONT && w.cont_fails.contains(&pid) {
            return Err(io::Error::other("fake"));
        }
        if sig == libc::SIGSTOP && w.fail_after_stop.contains(&pid) {
            w.read_fails.insert(pid);
        }
        if sig == libc::SIGSTOP {
            if let Some(c) = w.reuse_on_stop.remove(&pid) {
                w.next_unique += 1;
                let unique = w.next_unique;
                w.procs.insert(
                    pid,
                    P {
                        coalition: c,
                        stopped: true,
                        unique,
                        fork: false,
                    },
                );
                return Ok(());
            }
        }
        let stay = w.stay_stopped.contains(&pid);
        let Some(p) = w.procs.get_mut(&pid) else {
            return Err(io::Error::from_raw_os_error(libc::ESRCH));
        };
        match sig {
            libc::SIGSTOP => p.stopped = true,
            libc::SIGCONT if !stay => p.stopped = false,
            libc::SIGKILL => {
                let c = p.coalition;
                w.procs.remove(&pid);
                if let Some(n) = w.counts.get_mut(&c) {
                    n.1 += 1;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn run(&self, argv: &[OsString], _wait: Duration) -> io::Result<i32> {
        let program = argv[0].to_string_lossy().into_owned();
        if argv.get(1).is_some_and(|a| a == "bootout") {
            self.w()
                .bootouts
                .push(argv[2].to_string_lossy().into_owned());
            return Ok(3);
        }
        self.w()
            .exits
            .get(&program)
            .copied()
            .ok_or_else(|| io::Error::other("fake: no such program"))
    }

    fn uid(&self) -> u32 {
        501
    }
}

/// A data folder that the test removes at the end.
struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn setup(name: &str) -> (Dir, Seatbelt, Fake) {
    let d = std::env::temp_dir().join(format!("iris-macos-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    let fake = Fake::default();
    fake.w().boot = "100.000001".into();
    let s = Seatbelt::with(&d, Box::new(fake.clone())).unwrap();
    (Dir(d), s, fake)
}

fn rec(cid: u64) -> Record {
    Record {
        boot: "100.000001".into(),
        coalition: cid,
        port: Some(4000),
    }
}

#[test]
fn a_record_reads_back_and_a_bad_record_is_an_error() {
    let (_d, s, _f) = setup("record");
    assert_eq!(s.record(1).unwrap(), None);
    s.write_record(1, &rec(77)).unwrap();
    assert_eq!(s.record(1).unwrap(), Some(rec(77)));
    // The link refuses a second record of the same session.
    assert!(s.write_record(1, &rec(78)).is_err());
    let none = Record {
        port: None,
        ..rec(5)
    };
    s.write_record(2, &none).unwrap();
    assert_eq!(s.record(2).unwrap(), Some(none));
    for bad in [
        "",
        "1 2",
        "1.000001 0 -\n",
        "bad 50 4000\n",
        "1.0001 50 -\n",
        "1.000001 3 0\n",
        "1.000001 3 70000\n",
        "1.000001 3 -",
        "1.000001  3 -\n",
    ] {
        fs::write(s.file(3, "sandbox"), bad).unwrap();
        assert!(s.record(3).is_err(), "{bad:?}");
    }
}

#[test]
fn ended_is_true_only_with_no_live_process() {
    let (_d, s, f) = setup("ended");
    // No record: no agent ran.
    assert!(s.ended(1).unwrap());
    s.write_record(1, &rec(50)).unwrap();
    // The coalition is gone (ESRCH).
    assert!(s.ended(1).unwrap());
    let pid = f.add(50, false);
    assert!(!s.ended(1).unwrap());
    // An error of the counters is not an end.
    f.w().counts_fail = true;
    assert!(!s.ended(1).unwrap());
    f.w().counts_fail = false;
    f.signal(pid, libc::SIGKILL).unwrap();
    assert!(s.ended(1).unwrap());
    // Another boot: the system started again.
    f.add(50, false);
    assert!(!s.ended(1).unwrap());
    f.w().boot = "200.0".into();
    assert!(s.ended(1).unwrap());
}

#[test]
fn stop_freezes_a_fork_loop_then_kills_only_the_session() {
    let (_d, s, f) = setup("loop");
    s.write_record(1, &rec(50)).unwrap();
    for _ in 0..3 {
        f.add(50, true);
    }
    let other = f.add(60, true);
    s.stop(1).unwrap();
    assert!(s.ended(1).unwrap());
    assert_eq!(f.live(50), 0);
    assert_eq!(f.w().bootouts, [format!("gui/501/{}", s.name(1))]);
    // No signal reached a process outside the coalition.
    let w = f.w();
    assert!(
        w.signals.iter().all(|(p, _)| *p != other),
        "{:?}",
        w.signals
    );
    assert!(!w.procs[&other].stopped);
    drop(w);
    // The journal is empty after an Ok.
    assert!(s.read_journal(1).unwrap().is_empty());
}

#[test]
fn a_reused_process_id_during_the_freeze_gets_sigcont_and_lives() {
    let (_d, s, f) = setup("reuse");
    s.write_record(1, &rec(50)).unwrap();
    let pid = f.add(50, false);
    let q = f.add(50, false);
    // The process `pid` of the session ends, and its ID goes to a process
    // of coalition 60 just before the SIGSTOP.
    f.w().counts.get_mut(&50).unwrap().1 += 1;
    f.w().reuse_on_stop.insert(pid, 60);
    s.stop(1).unwrap();
    let w = f.w();
    let p = w.procs[&pid];
    assert_eq!(p.coalition, 60);
    assert!(!p.stopped, "the other process stays stopped");
    assert!(
        !w.signals.contains(&(pid, libc::SIGKILL)),
        "{:?}",
        w.signals
    );
    assert!(w.signals.contains(&(pid, libc::SIGCONT)), "{:?}", w.signals);
    assert!(!w.procs.contains_key(&q));
}

#[test]
fn a_journal_entry_of_another_stopped_process_gets_sigcont() {
    let (_d, s, f) = setup("journal");
    s.write_record(1, &rec(50)).unwrap();
    // A crash left an entry: the ID is now a stopped process of coalition 60.
    let pid = f.add(60, false);
    f.w().procs.get_mut(&pid).unwrap().stopped = true;
    s.journal(1, pid).unwrap();
    s.stop(1).unwrap();
    assert!(!f.w().procs[&pid].stopped);
    assert!(s.read_journal(1).unwrap().is_empty());
}

#[test]
fn an_entry_that_cannot_resume_stays_and_stop_fails() {
    let (_d, s, f) = setup("stays");
    s.write_record(1, &rec(50)).unwrap();
    let a = f.add(60, false);
    let b = f.add(61, false);
    for p in [a, b] {
        f.w().procs.get_mut(&p).unwrap().stopped = true;
        s.journal(1, p).unwrap();
    }
    f.w().cont_fails.insert(a);
    f.w().stay_stopped.insert(b);
    let e = s.stop(1).unwrap_err();
    assert!(e.to_string().contains("2 processes"), "{e}");
    let mut left = s.read_journal(1).unwrap();
    left.sort();
    assert_eq!(left, [a, b]);
    // A later stop resolves them when they can resume.
    f.w().cont_fails.clear();
    f.w().stay_stopped.clear();
    s.stop(1).unwrap();
    assert!(s.read_journal(1).unwrap().is_empty());
}

#[test]
fn stop_from_inside_the_coalition_sends_no_signal() {
    let (_d, s, f) = setup("inside");
    // The fake puts this process in coalition 1.
    s.write_record(1, &rec(1)).unwrap();
    f.add(1, false);
    s.stop(1).unwrap();
    let w = f.w();
    assert!(w.signals.is_empty() && w.bootouts.is_empty());
}

#[test]
fn stop_that_cannot_end_the_session_is_an_error() {
    let (_d, s, f) = setup("live");
    s.write_record(1, &rec(50)).unwrap();
    f.add(50, false);
    // The counters say one more task than the scan finds: no freeze settles.
    f.w().counts.get_mut(&50).unwrap().0 += 1;
    let e = s.stop_with(1, Duration::from_millis(50)).unwrap_err();
    assert!(e.to_string().contains("2 live processes"), "{e}");
    assert!(!s.ended(1).unwrap());
}

#[test]
fn the_label_holds_the_full_hash_of_the_real_data_folder() {
    let (d, s, _f) = setup("label");
    let link = d.0.with_extension("link");
    let _ = fs::remove_file(&link);
    std::os::unix::fs::symlink(&d.0, &link).unwrap();
    let t = Seatbelt::with(&link, Box::new(Fake::default())).unwrap();
    let _ = fs::remove_file(&link);
    let name = s.name(42);
    assert_eq!(name, t.name(42));
    let hash = name
        .strip_prefix("com.iris.session.")
        .and_then(|r| r.strip_suffix(".42"))
        .unwrap();
    assert_eq!(hash.len(), 64);
    assert!(hash.bytes().all(|b| b.is_ascii_hexdigit()));
}

fn paths(work: &str) -> Paths {
    Paths {
        workspace: PathBuf::from(work),
        home: "/data/sessions/se-1.home".into(),
        tmp: "/data/sessions/se-1.tmp".into(),
        programs: vec!["/Users/a/.local/bin/claude".into(), "/opt/iris/iris".into()],
    }
}

#[test]
fn the_profile_denies_by_default_and_allows_only_the_session() {
    let (text, params) = profile(&paths("/data/jobs/1/work"), Some(4000));
    assert!(text.starts_with("(version 1)\n(deny default)\n"));
    assert!(text.contains("(remote tcp \"localhost:4000\")"));
    assert!(!text.contains("remote ip"), "UDP must stay refused");
    assert!(!text.contains("kern.proc"));
    assert!(!text.contains("mach-lookup") && !text.contains("iokit-open"));
    assert!(!text.contains("(subpath \"/System\")") && !text.contains("(subpath \"/usr\")"));
    // The deny of the user volume comes after each allow but the folders
    // of the session.
    let deny = text
        .find("(deny file-read* file-write* (subpath \"/System/Volumes/Data\"))")
        .unwrap();
    let last = text.rfind("(allow ").unwrap();
    assert!(deny < last && text[last..].starts_with("(allow file-read* file-write*"));
    let p = |v: &str| {
        params
            .iter()
            .find(|(_, x)| x == Path::new(v))
            .unwrap()
            .0
            .clone()
    };
    assert!(text[last..].contains(&format!("(param \"{}\")", p("/data/jobs/1/work"))));
    assert!(text.contains(&format!(
        "(path-literal (param \"{}\"))",
        p("/data/jobs/1/work/.iris/socket")
    )));
    // The folder of a program only for its metadata, not its files.
    assert!(text.contains(&format!(
        "(literal (param \"{}\"))",
        p("/Users/a/.local/bin")
    )));
    assert!(!text.contains(&format!(
        "(subpath (param \"{}\"))",
        p("/Users/a/.local/bin")
    )));
    // No egress port: no TCP rule.
    let (text, _) = profile(&paths("/w"), None);
    assert!(!text.contains("remote tcp"));
}

/// The profile text with each parameter reference taken out.
fn strip_params(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find(" (literal (param \"P") {
        out.push_str(&rest[..i]);
        let end = rest[i..].find("\"))").unwrap();
        rest = &rest[i + end + 3..];
    }
    out.push_str(rest);
    let mut s = String::new();
    let mut rest = out.as_str();
    while let Some(i) = rest.find("(param \"P") {
        s.push_str(&rest[..i]);
        let end = rest[i..].find("\")").unwrap();
        let key = &rest[i + 8..i + end];
        assert!(
            key.len() > 1 && key[1..].bytes().all(|b| b.is_ascii_digit()),
            "{key}"
        );
        s.push_str("(param)");
        rest = &rest[i + end + 2..];
    }
    s.push_str(rest);
    s
}

/// No path can change the profile text: each path goes in a parameter.
#[test]
fn no_path_reaches_the_profile_text() {
    let mut seed = 0x9e37_79b9_7f4a_7c15u64;
    let chars: Vec<char> = "a/\")( \n\\;#|.".chars().collect();
    let (base, _) = profile(&paths("/w"), Some(1));
    let base = strip_params(&base);
    for _ in 0..300 {
        let mut w = String::from("/");
        for _ in 0..12 {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            w.push(chars[(seed >> 33) as usize % chars.len()]);
        }
        let (text, params) = profile(&paths(&w), Some(1));
        assert_eq!(strip_params(&text), base, "{w:?}");
        assert!(params.iter().any(|(_, v)| v == Path::new(&w)));
    }
}

#[test]
fn a_failed_read_after_a_sigstop_keeps_the_entry_and_fails_the_stop() {
    let (_d, s, f) = setup("unknown");
    s.write_record(1, &rec(50)).unwrap();
    let pid = f.add(50, false);
    // The process of the session ends, its ID goes to a process of
    // coalition 60 at the SIGSTOP, and each later read of it fails.
    f.w().counts.get_mut(&50).unwrap().1 += 1;
    let q = f.add(50, false);
    f.w().reuse_on_stop.insert(pid, 60);
    f.w().fail_after_stop.insert(pid);
    let e = s.stop(1).unwrap_err();
    assert!(e.to_string().contains("1 processes"), "{e}");
    assert_eq!(s.read_journal(1).unwrap(), [pid]);
    assert!(!f.w().procs.contains_key(&q));
    // When the read works again, the next stop sends SIGCONT and resolves it.
    f.w().read_fails.clear();
    s.stop(1).unwrap();
    assert!(!f.w().procs[&pid].stopped);
    assert!(s.read_journal(1).unwrap().is_empty());
}

#[test]
fn check_needs_the_sandbox_the_gui_domain_and_the_counters() {
    let (_d, s, f) = setup("check");
    let ok = |f: &Fake| {
        let mut w = f.w();
        w.exits.insert(SANDBOX_EXEC.into(), 0);
        w.exits.insert("/bin/launchctl".into(), 0);
        w.counts.insert(1, (3, 1));
        w.counts_fail = false;
    };
    ok(&f);
    s.check().unwrap();
    f.w().exits.insert(SANDBOX_EXEC.into(), 65);
    assert!(s.check().unwrap_err().contains("sandbox-exec"));
    f.w().exits.remove(SANDBOX_EXEC);
    assert!(s.check().unwrap_err().contains("sandbox-exec"));
    ok(&f);
    f.w().exits.insert("/bin/launchctl".into(), 113);
    assert!(s.check().unwrap_err().contains("GUI domain"));
    ok(&f);
    f.w().counts.remove(&1);
    assert!(s.check().unwrap_err().contains("gone"));
    ok(&f);
    f.w().counts_fail = true;
    assert!(s.check().unwrap_err().contains("counters"));
}

/// Random text: a record parses only as the oracle says, and each valid
/// record reads back the same.
#[test]
fn no_random_text_is_a_record_but_the_valid_form() {
    fn oracle(t: &str) -> bool {
        let Some(line) = t.strip_suffix('\n') else {
            return false;
        };
        let p: Vec<&str> = line.split(' ').collect();
        let num = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
        p.len() == 3
            && p[0]
                .split_once('.')
                .is_some_and(|(a, b)| num(a) && num(b) && b.len() == 6)
            && num(p[1])
            && p[1].parse::<u64>().is_ok_and(|c| c > 0)
            && (p[2] == "-" || (num(p[2]) && p[2].parse::<u16>().is_ok_and(|x| x > 0)))
    }
    let mut seed = 11u64;
    let mut next = |n: u64| {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) % n
    };
    let alphabet = b"0123456789. -\nx";
    let mut valid = 0;
    for _ in 0..50_000 {
        let t: String = if next(2) == 0 {
            let len = next(24);
            (0..len)
                .map(|_| alphabet[next(alphabet.len() as u64) as usize] as char)
                .collect()
        } else {
            let mut t = Record {
                boot: format!("{}.{:06}", next(1 << 40), next(1_000_000)),
                coalition: next(1 << 20),
                port: (next(3) > 0).then(|| next(70_000) as u16),
            }
            .to_line();
            // Cut or change one byte.
            if next(3) == 0 {
                let i = next(t.len() as u64) as usize;
                t.replace_range(
                    i..i + 1,
                    &(alphabet[next(alphabet.len() as u64) as usize] as char).to_string(),
                );
            }
            t
        };
        let got = Record::parse(&t);
        assert_eq!(got.is_some(), oracle(&t), "{t:?}");
        if let Some(r) = got {
            valid += 1;
            assert_eq!(Record::parse(&r.to_line()), Some(r));
        }
    }
    assert!(valid > 5000, "{valid}");
}

/// Random text: the journal parses only as one positive process ID on each
/// whole line.
#[test]
fn no_random_text_is_a_journal_but_the_valid_form() {
    let mut seed = 5u64;
    let mut next = |n: u64| {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (seed >> 33) % n
    };
    let alphabet = b"0123456789\n -x";
    for _ in 0..50_000 {
        let t: String = if next(2) == 0 {
            let len = next(20);
            (0..len)
                .map(|_| alphabet[next(alphabet.len() as u64) as usize] as char)
                .collect()
        } else {
            (0..next(4))
                .map(|_| format!("{}\n", next(1 << 31)))
                .collect()
        };
        let lines: Option<Vec<i32>> = match t.rfind('\n') {
            None => Some(Vec::new()),
            Some(i) => Some(&t[..i]).and_then(|b| {
                b.split('\n')
                    .map(|l| {
                        (!l.is_empty() && l.bytes().all(|c| c.is_ascii_digit()))
                            .then(|| l.parse::<i32>().ok().filter(|p| *p > 0))
                            .flatten()
                    })
                    .collect()
            }),
        };
        assert_eq!(parse_journal(&t), lines, "{t:?}");
        if let Some(p) = &lines {
            let back: String = p.iter().map(|x| format!("{x}\n")).collect();
            assert_eq!(parse_journal(&back).as_ref(), Some(p));
        }
    }
}

#[test]
fn a_journal_of_an_earlier_boot_is_removed_and_resumes_nothing() {
    let (_d, s, f) = setup("reboot");
    s.write_record(1, &rec(50)).unwrap();
    // After a restart the ID of the entry belongs to a stopped process
    // of the person.
    let pid = f.add(60, false);
    f.w().procs.get_mut(&pid).unwrap().stopped = true;
    s.journal(1, pid).unwrap();
    f.w().boot = "200.000000".into();
    s.stop(1).unwrap();
    let w = f.w();
    assert!(
        w.signals.is_empty() && w.bootouts.is_empty(),
        "{:?}",
        w.signals
    );
    assert!(w.procs[&pid].stopped);
    drop(w);
    assert!(!s.file(1, "stops").exists());
}

#[test]
fn a_cut_last_journal_line_is_left_out_and_the_rest_resolves() {
    let (_d, s, f) = setup("cut");
    s.write_record(1, &rec(50)).unwrap();
    // A crash left one whole entry of a stopped process of coalition 60,
    // then a cut write with no end of line.
    let pid = f.add(60, false);
    f.w().procs.get_mut(&pid).unwrap().stopped = true;
    fs::write(s.file(1, "stops"), format!("{pid}\n12")).unwrap();
    assert_eq!(s.read_journal(1).unwrap(), [pid]);
    s.stop(1).unwrap();
    assert!(!f.w().procs[&pid].stopped);
    assert!(s.read_journal(1).unwrap().is_empty());
}

#[test]
fn the_command_runs_the_program_in_the_profile_with_the_private_folders() {
    let (d, s, _f) = setup("command");
    let work = d.0.join("work");
    fs::create_dir_all(&work).unwrap();
    let program = d.0.join("tool");
    fs::write(&program, "").unwrap();
    let c = s.command(
        1,
        &work,
        Some(4100),
        &program,
        &["--task".to_string(), "a b".to_string()],
    );
    assert_eq!(c.program, PathBuf::from(SANDBOX_EXEC));
    let a: Vec<String> = c
        .args
        .iter()
        .map(|x| x.to_string_lossy().into_owned())
        .collect();
    let env = a.iter().position(|x| x == "/usr/bin/env").unwrap();
    // The options of sandbox-exec: the parameters, then the profile.
    let p = a.iter().position(|x| x == "-p").unwrap();
    assert_eq!(p + 2, env);
    assert!(a[p + 1].starts_with("(version 1)\n(deny default)"));
    assert!(a[p + 1].contains("localhost:4100"));
    assert!(a[..p]
        .chunks(2)
        .all(|o| o[0] == "-D" && o[1].starts_with('P')));
    let real = |x: &Path| fs::canonicalize(x).unwrap().to_string_lossy().into_owned();
    let work_param = a[..p]
        .iter()
        .any(|o| o.ends_with(&format!("={}", real(&work))));
    assert!(work_param, "{a:?}");
    // env: the private home and /tmp, the folder of iris first in PATH,
    // then the program and its arguments.
    let home = s.home(1).to_string_lossy().into_owned();
    let tmp = s.tmp(1).to_string_lossy().into_owned();
    assert_eq!(a[env + 1], format!("HOME={home}"));
    assert_eq!(a[env + 2], format!("TMPDIR={tmp}"));
    let exe = fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
    let dir = exe.parent().unwrap().to_string_lossy().into_owned();
    assert!(
        a[env + 3].starts_with(&format!("PATH={dir}")),
        "{}",
        a[env + 3]
    );
    assert_eq!(
        a[env + 4..],
        [real(&program), "--task".into(), "a b".into()]
    );
}
