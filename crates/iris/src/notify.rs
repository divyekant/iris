//! The system notice of a notice by rule (card R5-08,
//! `docs/specs/2026-11-16-notices.md`): `osascript` on macOS, `notify-send`
//! on Linux. The words of the mail go as arguments, never in a script or a
//! shell line: mail text is data.

use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
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

/// The time of the last `app.notices` call, in milliseconds since 1970. 0
/// when no app asked.
static APP_ASKED: AtomicI64 = AtomicI64::new(0);

/// The largest notice row ID that the app does not show: the `last` of its
/// first call, which gives no notices. `i64::MAX` when no app asked.
static APP_FROM: AtomicI64 = AtomicI64::new(i64::MAX);

/// Record an `app.notices` call at `now_ms`. `from` is the `last` of a first
/// call (no `after`): the app shows only the notices after it.
pub fn app_asked(now_ms: i64, from: Option<i64>) {
    if let Some(f) = from {
        APP_FROM.store(f, Ordering::Relaxed);
    }
    APP_ASKED.fetch_max(now_ms, Ordering::Relaxed);
}

/// True when the app shows notice `id`, so the engine does not: an app
/// asked [`APP_WAIT_MS`] or less before `now_ms` (the call at `asked_ms`),
/// and `id` is after the start `from` of the app. Each notice is shown by the
/// app or by the engine, also a notice that came during the first call.
pub fn app_shows(id: i64, from: i64, asked_ms: i64, now_ms: i64) -> bool {
    asked_ms > 0 && now_ms - asked_ms <= APP_WAIT_MS && id > from
}

/// [`app_shows`] with the calls that the engine recorded.
pub fn app_shows_now(id: i64, now_ms: i64) -> bool {
    app_shows(
        id,
        APP_FROM.load(Ordering::Relaxed),
        APP_ASKED.load(Ordering::Relaxed),
        now_ms,
    )
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
        let shows = |asked| app_shows(5, 4, asked, t);
        assert!(!shows(0));
        assert!(shows(t - 10_000));
        assert!(shows(t - APP_WAIT_MS));
        assert!(!shows(t - APP_WAIT_MS - 1));
        assert!(!shows(t - 31_000));
        // A notice at or before the start of the app: the engine shows it.
        assert!(!app_shows(4, 4, t, t));
        assert!(!app_shows(3, 4, t, t));
        assert!(!app_shows(1, i64::MAX, t, t));
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
