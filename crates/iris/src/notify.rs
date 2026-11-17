//! The system notice of a notice by rule (card R5-08,
//! `docs/specs/2026-11-16-notices.md`): `osascript` on macOS, `notify-send`
//! on Linux. The words of the mail go as arguments, never in a script or a
//! shell line: mail text is data.

use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// The longest wait for the notice command.
pub const WAIT: Duration = Duration::from_secs(5);

/// The command that shows one system notice on the system `os` (the value of
/// `std::env::consts::OS`). `None` on a system with no known command. The
/// title must not start with `-` (the caller starts it with "Iris:"):
/// `osascript` reads such an argument as an option.
pub fn command(os: &str, title: &str, text: &str) -> Option<Command> {
    let mut c = match os {
        "macos" => {
            let mut c = Command::new("/usr/bin/osascript");
            // A fixed script. The words are `argv`, so no text of the mail
            // becomes AppleScript.
            c.args([
                "-e",
                "on run argv",
                "-e",
                "display notification (item 2 of argv) with title (item 1 of argv)",
                "-e",
                "end run",
            ]);
            c
        }
        "linux" => {
            let mut c = Command::new("notify-send");
            // `--` so that a word that starts with `-` is not an option.
            c.args(["--app-name=Iris", "--"]);
            c
        }
        _ => return None,
    };
    c.args([title, text])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    Some(c)
}

/// Runs `c` and waits at most `wait`. Kills it after that.
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
            return Err(format!("no answer in {} seconds", wait.as_secs()));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The most notices that wait for their system notice. More are dropped:
/// the web client still shows them.
pub const QUEUE: usize = 100;

/// Starts the thread that shows each notice that it gets with `show`, one
/// at a time. The sender never waits: a slow command must not hold the
/// attention thread (spec: the pass runs each 10 seconds).
pub fn spawn(
    show: fn(&iris_policy::notice::Notice),
) -> std::sync::mpsc::SyncSender<iris_policy::notice::Notice> {
    let (tx, rx) = std::sync::mpsc::sync_channel::<iris_policy::notice::Notice>(QUEUE);
    let _ = std::thread::Builder::new()
        .name("notices".into())
        .spawn(move || rx.iter().for_each(|n| show(&n)));
    tx
}

/// The longest time since the last `app.notices` call of the Mac app, in
/// which the app shows the notices and the engine does not
/// (`docs/specs/2026-11-17-mac-app.md`). The app asks each 10 seconds.
pub const APP_WAIT_MS: i64 = 30_000;

/// The state of the Mac app in the engine (in memory only).
struct App {
    /// The time of the last `app.notices` call, in milliseconds since 1970.
    /// 0 when no app asked.
    asked_ms: i64,
    /// The largest notice row ID that the app does not show: the `last` of
    /// the call that started the app. `i64::MAX` when no app asked.
    from: i64,
    /// The largest notice row ID that the app received: the largest `last`
    /// of its answers. The app shows each notice after `from` up to it.
    seen: i64,
}

/// One lock for the call of the app and the choice of the engine: each
/// notice has one owner, the app or the engine.
static APP: std::sync::Mutex<App> = std::sync::Mutex::new(App {
    asked_ms: 0,
    from: i64::MAX,
    seen: i64::MIN,
});

fn app() -> std::sync::MutexGuard<'static, App> {
    APP.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Run one `app.notices` call at `now_ms` under the lock of [`show`].
/// `read` gets `true` when the app owns the notices after its start: its
/// last call came [`APP_WAIT_MS`] or less before. With `false` (the first
/// call, or a call after a gap), the call starts the app again. `read` gives
/// the answer, its `last`, and `true` when the call started the app: the app
/// shows only the notices after that `last`, and the engine shows all
/// others. A notice that the engine shows before the lock is in the store
/// before `read`, so its row ID is at or below that `last`.
pub fn app_call<T, E>(
    now_ms: i64,
    read: impl FnOnce(bool) -> Result<(T, i64, bool), E>,
) -> Result<T, E> {
    let mut a = app();
    let (answer, last, start) = read(app_live(a.asked_ms, now_ms))?;
    if start {
        a.from = last;
        a.seen = last;
    }
    a.seen = a.seen.max(last);
    a.asked_ms = a.asked_ms.max(now_ms);
    Ok(answer)
}

/// True when an app asked at `asked_ms`, [`APP_WAIT_MS`] or less before
/// `now_ms`.
fn app_live(asked_ms: i64, now_ms: i64) -> bool {
    asked_ms > 0 && now_ms - asked_ms <= APP_WAIT_MS
}

/// True when the app shows notice `id`, so the engine does not: `id` is
/// after the start `from` of the app, and the app received it (`seen`) or
/// is live (its call at `asked_ms`). A notice that the app received stays
/// with the app also when it waits long in the queue of [`spawn`].
pub fn app_shows(id: i64, from: i64, seen: i64, asked_ms: i64, now_ms: i64) -> bool {
    id > from && (id <= seen || app_live(asked_ms, now_ms))
}

/// [`app_shows`] with the state that the engine recorded, under the lock of
/// [`app_call`].
pub fn app_shows_now(id: i64, now_ms: i64) -> bool {
    let a = app();
    app_shows(id, a.from, a.seen, a.asked_ms, now_ms)
}

/// True after the first failure, so that a server with no desktop writes
/// one log line, not one for each notice.
static FAILED: AtomicBool = AtomicBool::new(false);

/// Shows one system notice for `n`. A failure writes one log line the first
/// time. The notice stays in the web client either way.
pub fn show(n: &iris_policy::notice::Notice) {
    if app_shows_now(n.id, crate::engine::now_ms()) {
        return;
    }
    let title = format!("Iris: {}", n.rule_name);
    let text = format!("{}: {}", n.title, n.text);
    let result = match command(std::env::consts::OS, &title, &text) {
        Some(c) => run(c, WAIT),
        None => Err("this system has no notice command".into()),
    };
    if let Err(e) = result {
        if !FAILED.swap(true, Ordering::Relaxed) {
            say!("iris: notices: no system notice ({e}). The web client shows the notices.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(c: &Command) -> Vec<String> {
        c.get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn system_notice_passes_words_as_arguments() {
        let evil = r#"" & do shell script "rm -rf ~" & "$(touch /tmp/x)`id`"#;
        for os in ["macos", "linux"] {
            let c = command(os, "-t", evil).unwrap();
            let a = args(&c);
            // The words are the last two arguments, as they are.
            assert_eq!(&a[a.len() - 2..], ["-t", evil], "{os}");
            // No other argument holds a word of the mail.
            assert!(a[..a.len() - 2].iter().all(|x| !x.contains("rm -rf")));
        }
        let mac = command("macos", "t", "x").unwrap();
        assert_eq!(mac.get_program(), "/usr/bin/osascript");
        assert_eq!(args(&command("linux", "t", "x").unwrap())[1], "--");
        assert!(command("windows", "t", "x").is_none());
    }

    fn notice(id: i64) -> iris_policy::notice::Notice {
        iris_policy::notice::Notice {
            id,
            message_id: id,
            rule: 1,
            rule_name: "r".into(),
            title: "t".into(),
            text: "x".into(),
            created_at: 0,
        }
    }

    #[test]
    fn slow_system_notices_never_hold_the_sender() {
        // Each notice takes 1 second to show. The sends still end at once.
        let tx = spawn(|_| std::thread::sleep(Duration::from_secs(1)));
        let t = Instant::now();
        for id in 0..(QUEUE as i64 + 20) {
            let _ = tx.try_send(notice(id));
        }
        assert!(
            t.elapsed() < Duration::from_millis(500),
            "{:?}",
            t.elapsed()
        );
    }

    #[test]
    fn the_app_shows_the_notices_for_30_seconds_after_its_call() {
        let t = 1_800_000_000_000;
        let shows = |asked| app_shows(5, 4, 4, asked, t);
        assert!(!shows(0));
        assert!(shows(t - 10_000));
        assert!(shows(t - APP_WAIT_MS));
        assert!(!shows(t - APP_WAIT_MS - 1));
        assert!(!shows(t - 31_000));
        // A notice at or before the start of the app: the engine shows it.
        assert!(!app_shows(4, 4, 4, t, t));
        assert!(!app_shows(3, 4, 4, t, t));
        assert!(!app_shows(1, i64::MAX, i64::MIN, t, t));
        // A notice that the app received stays with the app after the gap.
        assert!(app_shows(5, 4, 5, 0, t));
        assert!(!app_shows(6, 4, 5, t - 31_000, t));
    }

    #[test]
    fn each_notice_of_the_first_call_has_one_owner() {
        // The only test of the state of the app in this binary.
        let now = 1_800_000_000_000;
        // Notice 5 comes while the first call reads the store, after the
        // read: the engine choice waits for the call, then leaves notice 5
        // to the app.
        let engine = app_call(now, |live| {
            assert!(!live, "no app asked before");
            let engine = std::thread::spawn(move || app_shows_now(5, now));
            std::thread::sleep(Duration::from_millis(100));
            assert!(!engine.is_finished(), "the choice waits for the call");
            Ok::<_, ()>((engine, 4, true))
        })
        .unwrap();
        assert!(engine.join().unwrap(), "the app shows notice 5");
        // Notice 4 was in the store before the read: the app does not show
        // it (its `last` is 4), so the engine does.
        assert!(!app_shows_now(4, now));
        // A later call of the live app keeps its start.
        let next = now + 10_000;
        // The app received notice 5. A long wait in the queue of the engine
        // does not give notice 5 to the engine after a gap.
        assert!(app_call(next, |live| Ok::<_, ()>((live, 5, false))).unwrap());
        assert!(app_shows_now(5, next));
        assert!(app_shows_now(5, next + APP_WAIT_MS * 10));
        // After a gap the engine shows the notices, and the next call
        // starts the app again after its new `last`.
        let gap = next + APP_WAIT_MS + 1;
        assert!(!app_shows_now(6, gap));
        assert!(!app_call(gap, |live| Ok::<_, ()>((live, 6, true))).unwrap());
        assert!(!app_shows_now(6, gap));
        assert!(app_shows_now(7, gap));
        // A failed read records nothing.
        let late = gap + APP_WAIT_MS + 1;
        assert!(app_call(late, |_| Err::<((), i64, bool), _>(())).is_err());
        assert!(!app_shows_now(7, late));
    }

    #[test]
    fn a_missing_or_slow_command_is_an_error() {
        assert!(run(Command::new("/no/such/iris-notice"), WAIT).is_err());
        let mut slow = Command::new("sleep");
        slow.arg("5");
        let t = Instant::now();
        assert!(run(slow, Duration::from_millis(200)).is_err());
        assert!(t.elapsed() < Duration::from_secs(3));
        assert!(run(Command::new("true"), WAIT).is_ok());
        assert!(run(Command::new("false"), WAIT).is_err());
    }
}
