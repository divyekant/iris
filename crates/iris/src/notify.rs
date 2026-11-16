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

/// True after the first failure, so that a server with no desktop writes
/// one log line, not one for each notice.
static FAILED: AtomicBool = AtomicBool::new(false);

/// Shows one system notice for `n`. A failure writes one log line the first
/// time. The notice stays in the web client either way.
pub fn show(n: &iris_policy::notice::Notice) {
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
