//! The background service of Iris.
//!
//! On macOS the service is a launchd agent in `~/Library/LaunchAgents`. On
//! Linux it is a systemd user unit in `~/.config/systemd/user`. Both run
//! `iris serve` when the user logs in, and again after a crash.
//!
//! [`launchd_plist`] and [`systemd_unit`] make the file text and do no I/O.
//! [`install`] and [`remove`] write or delete the file and tell the service
//! manager.

use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The launchd label of the agent. The plist file is `<LABEL>.plist`.
pub const LABEL: &str = "io.github.divyekant.iris";

/// The name of the systemd user unit.
pub const UNIT: &str = "iris.service";

/// A service manager.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum System {
    /// macOS launchd, as a per-user agent.
    Launchd,
    /// Linux systemd, as a user unit.
    Systemd,
}

impl System {
    /// The service manager of this system, or `None` if Iris has none for it.
    pub fn current() -> Option<System> {
        if cfg!(target_os = "macos") {
            Some(System::Launchd)
        } else if cfg!(target_os = "linux") {
            Some(System::Systemd)
        } else {
            None
        }
    }

    /// The path of the service file under the folders of the user.
    ///
    /// `config` is the systemd config folder (`$XDG_CONFIG_HOME`, else
    /// `~/.config`). launchd does not use it.
    pub fn file(self, home: &Path, config: &Path) -> PathBuf {
        match self {
            System::Launchd => home
                .join("Library/LaunchAgents")
                .join(format!("{LABEL}.plist")),
            System::Systemd => config.join("systemd/user").join(UNIT),
        }
    }

    /// The text of the service file that runs `<program> serve`.
    pub fn text(self, program: &Path, home: &Path) -> io::Result<String> {
        match self {
            System::Launchd => launchd_plist(program, &home.join("Library/Logs/iris.log")),
            System::Systemd => systemd_unit(program),
        }
    }
}

/// The text of the launchd agent that runs `<program> serve`.
///
/// launchd starts the agent at login and starts it again if it stops with an
/// error. Its output goes to `log`.
pub fn launchd_plist(program: &Path, log: &Path) -> io::Result<String> {
    let program = xml(checked(program)?);
    let log = xml(checked(log)?);
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>Label</key>
	<string>{LABEL}</string>
	<key>ProgramArguments</key>
	<array>
		<string>{program}</string>
		<string>serve</string>
	</array>
	<key>RunAtLoad</key>
	<true/>
	<key>KeepAlive</key>
	<dict>
		<key>SuccessfulExit</key>
		<false/>
	</dict>
	<key>ThrottleInterval</key>
	<integer>10</integer>
	<key>StandardOutPath</key>
	<string>{log}</string>
	<key>StandardErrorPath</key>
	<string>{log}</string>
</dict>
</plist>
"#
    ))
}

/// The text of the systemd user unit that runs `<program> serve`.
///
/// systemd starts the unit at login (or at boot, if lingering is on for the
/// user) and starts it again if it stops with an error. The output goes to
/// the journal.
pub fn systemd_unit(program: &Path) -> io::Result<String> {
    let program = checked(program)?;
    // systemd reads `%` as a specifier and `$` as a variable, and a quoted
    // word takes C escapes.
    let mut quoted = String::from('"');
    for c in program.chars() {
        match c {
            '"' => quoted.push_str("\\\""),
            '\\' => quoted.push_str("\\\\"),
            '%' => quoted.push_str("%%"),
            '$' => quoted.push_str("$$"),
            c => quoted.push(c),
        }
    }
    quoted.push('"');
    Ok(format!(
        "[Unit]
Description=Iris mail engine
After=network-online.target
Wants=network-online.target

[Service]
ExecStart={quoted} serve
Restart=on-failure
RestartSec=10

[Install]
WantedBy=default.target
"
    ))
}

/// A path that is safe to put in a service file: absolute, UTF-8, and with
/// no control characters (a new line would add a line to the file).
fn checked(path: &Path) -> io::Result<&str> {
    let invalid = |why: &str| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{why}: {}", path.display()),
        )
    };
    if !path.is_absolute() {
        return Err(invalid("the path is not absolute"));
    }
    let text = path
        .to_str()
        .ok_or_else(|| invalid("the path is not UTF-8"))?;
    if text.chars().any(char::is_control) {
        return Err(invalid("the path has a control character"));
    }
    Ok(text)
}

fn xml(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Write the service file for `program` under `home` and `config`, with its
/// folder. Return its path. This does not tell the service manager.
pub fn write(system: System, program: &Path, home: &Path, config: &Path) -> io::Result<PathBuf> {
    let text = system.text(program, home)?;
    let file = system.file(home, config);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if system == System::Launchd {
        std::fs::create_dir_all(home.join("Library/Logs"))?;
    }
    // Write a new file, then rename it, so that a stop never leaves half a file.
    let new = file.with_extension("new");
    std::fs::write(&new, text)?;
    std::fs::rename(&new, &file)?;
    Ok(file)
}

/// Install the service for `program` (an absolute path to the iris program)
/// and start it now. A second install replaces the first. Return the path of
/// the service file.
pub fn install(program: &Path) -> io::Result<PathBuf> {
    let system = unsupported(System::current())?;
    let (home, config) = folders()?;
    let file = write(system, program, &home, &config)?;
    match system {
        System::Launchd => {
            let domain = format!("gui/{}", uid()?);
            // Stop the old agent first. This fails if none is loaded.
            let _ = output("launchctl", &["bootout", &format!("{domain}/{LABEL}")]);
            run("launchctl", &["bootstrap", &domain, path_str(&file)?])?;
        }
        System::Systemd => {
            run("systemctl", &["--user", "daemon-reload"])?;
            run("systemctl", &["--user", "enable", UNIT])?;
            run("systemctl", &["--user", "restart", UNIT])?;
        }
    }
    Ok(file)
}

/// Stop the service and delete its file. Return the path of the deleted file,
/// or `None` if no service was installed.
pub fn remove() -> io::Result<Option<PathBuf>> {
    let system = unsupported(System::current())?;
    let (home, config) = folders()?;
    let file = system.file(&home, &config);
    if !file.exists() {
        return Ok(None);
    }
    match system {
        System::Launchd => {
            let _ = output(
                "launchctl",
                &["bootout", &format!("gui/{}/{LABEL}", uid()?)],
            );
            std::fs::remove_file(&file)?;
        }
        System::Systemd => {
            let _ = output("systemctl", &["--user", "disable", "--now", UNIT]);
            std::fs::remove_file(&file)?;
            run("systemctl", &["--user", "daemon-reload"])?;
        }
    }
    Ok(Some(file))
}

fn unsupported(system: Option<System>) -> io::Result<System> {
    system.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::Unsupported,
            "Iris has no background service for this system",
        )
    })
}

/// The home folder and the systemd config folder of the user.
fn folders() -> io::Result<(PathBuf, PathBuf)> {
    let var = |name| {
        std::env::var_os(name)
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
    };
    let home = var("HOME").filter(|h| h.is_absolute()).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "HOME is not set to an absolute path",
        )
    })?;
    // The XDG rules say to ignore a relative path.
    let config = var("XDG_CONFIG_HOME")
        .filter(|c| c.is_absolute())
        .unwrap_or_else(|| home.join(".config"));
    Ok((home, config))
}

fn path_str(path: &Path) -> io::Result<&str> {
    path.to_str().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("the path is not UTF-8: {}", path.display()),
        )
    })
}

fn uid() -> io::Result<String> {
    let out = run("id", &["-u"])?;
    Ok(out.trim().to_string())
}

fn output(program: &str, args: &[&str]) -> io::Result<std::process::Output> {
    Command::new(program).args(args).output()
}

/// Run a command. Return its output, or an error with its message if it fails.
fn run(program: &str, args: &[&str]) -> io::Result<String> {
    let out = output(program, args)
        .map_err(|e| io::Error::new(e.kind(), format!("cannot run {program}: {e}")))?;
    if !out.status.success() {
        let why = String::from_utf8_lossy(&out.stderr);
        return Err(io::Error::other(format!(
            "{program} {} failed: {}",
            args.join(" "),
            why.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_runs_serve_and_escapes_the_path() {
        let text = launchd_plist(
            Path::new("/Users/a&b/<x>/iris"),
            Path::new("/Users/a/Library/Logs/iris.log"),
        )
        .unwrap();
        assert!(text.contains(
            "<string>/Users/a&amp;b/&lt;x&gt;/iris</string>\n\t\t<string>serve</string>"
        ));
        assert!(text.contains(&format!("<string>{LABEL}</string>")));
        assert!(text.contains("<key>RunAtLoad</key>\n\t<true/>"));
        assert!(text.contains("<key>SuccessfulExit</key>\n\t\t<false/>"));
        assert!(text.contains("<string>/Users/a/Library/Logs/iris.log</string>"));
        assert!(!text.contains("a&b"));
    }

    #[test]
    fn unit_runs_serve_and_quotes_the_path() {
        let text = systemd_unit(Path::new("/home/a b/50%/$x/\"q\"\\/iris")).unwrap();
        assert!(
            text.contains("\nExecStart=\"/home/a b/50%%/$$x/\\\"q\\\"\\\\/iris\" serve\n"),
            "{text}"
        );
        assert!(text.contains("\nRestart=on-failure\n"));
        assert!(text.contains("\nWantedBy=default.target\n"));
    }

    #[test]
    fn a_bad_path_is_refused() {
        for bad in [
            "iris",
            "/home/a\nExecStartPre=/bin/evil/iris",
            "/home/a\r/iris",
        ] {
            assert!(systemd_unit(Path::new(bad)).is_err(), "{bad:?}");
            assert!(
                launchd_plist(Path::new(bad), Path::new("/l")).is_err(),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn files_go_to_the_user_folders() {
        let (home, config) = (Path::new("/h"), Path::new("/c"));
        assert_eq!(
            System::Launchd.file(home, config),
            Path::new("/h/Library/LaunchAgents/io.github.divyekant.iris.plist")
        );
        assert_eq!(
            System::Systemd.file(home, config),
            Path::new("/c/systemd/user/iris.service")
        );
    }

    #[test]
    fn write_makes_the_file_and_replaces_it() {
        let dir = std::env::temp_dir().join(format!("iris-service-write-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (home, config) = (dir.join("home"), dir.join("config"));
        for system in [System::Launchd, System::Systemd] {
            let first = write(system, Path::new("/old/iris"), &home, &config).unwrap();
            let file = write(system, Path::new("/new/iris"), &home, &config).unwrap();
            assert_eq!(first, file);
            let text = std::fs::read_to_string(&file).unwrap();
            assert!(text.contains("/new/iris") && !text.contains("/old/iris"));
            assert!(!file.with_extension("new").exists());
        }
        assert!(home.join("Library/Logs").is_dir());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
