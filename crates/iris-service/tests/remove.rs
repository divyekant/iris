//! A failed install keeps the service file and returns the error. `remove`
//! keeps the service file when the stop fails, so that a second remove can
//! try again, and deletes it when the system has no systemctl. A fake `systemctl` on PATH stands in for systemd.
//! This file has one test, because the test changes HOME and PATH.
#![cfg(target_os = "linux")]

use std::path::Path;

fn fake(bin: &Path, body: &str) {
    let file = bin.join("systemctl");
    std::fs::write(
        &file,
        format!("#!/bin/sh\necho \"$@\" >> \"$HOME/calls\"\n{body}\n"),
    )
    .unwrap();
    let ok = std::process::Command::new("chmod")
        .arg("755")
        .arg(&file)
        .status()
        .unwrap()
        .success();
    assert!(ok);
}

#[test]
fn failed_service_manager_calls_leave_a_file_state_that_a_retry_can_fix() {
    let dir = std::env::temp_dir().join(format!("iris-service-remove-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (home, bin) = (dir.join("home"), dir.join("bin"));
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&bin).unwrap();
    std::env::set_var("HOME", &home);
    std::env::remove_var("XDG_CONFIG_HOME");
    let path = std::env::var("PATH").unwrap_or_default();
    std::env::set_var("PATH", format!("{}:{path}", bin.display()));

    assert_eq!(
        iris_service::remove().unwrap(),
        None,
        "no file, nothing to stop"
    );
    assert!(!home.join("calls").exists());

    // A failed install returns the error and keeps the unit file, so that
    // a second install or a remove can finish the work.
    let unit = home.join(".config/systemd/user/iris.service");
    fake(&bin, "echo 'Failed to connect to bus' >&2; exit 1");
    let err = iris_service::install(Path::new("/opt/iris"), Path::new("/srv/iris")).unwrap_err();
    assert!(
        err.to_string().contains("Failed to connect to bus"),
        "{err}"
    );
    assert!(unit.exists());

    // With no systemctl found: remove deletes the unit file only if systemd
    // does not run on this machine. Else the file stays and remove fails.
    std::fs::remove_file(bin.join("systemctl")).unwrap();
    std::env::set_var("PATH", &bin);
    if Path::new("/run/systemd/system").is_dir() {
        assert!(iris_service::remove().is_err());
        assert!(unit.exists());
        std::fs::remove_file(&unit).unwrap();
    } else {
        assert_eq!(iris_service::remove().unwrap(), Some(unit.clone()));
        assert!(!unit.exists());
    }
    std::env::set_var("PATH", format!("{}:{path}", bin.display()));
    std::fs::remove_file(home.join("calls")).unwrap();

    let file = iris_service::write(
        iris_service::System::Systemd,
        Path::new("/opt/iris"),
        Path::new("/srv/iris"),
        &home,
        &home.join(".config"),
    )
    .unwrap();

    // The stop runs only when systemd runs on this machine (the unit tests
    // of remove_unit cover both cases).
    if Path::new("/run/systemd/system").is_dir() {
        fake(
            &bin,
            "case \"$*\" in *disable*) echo 'Failed to connect to bus' >&2; exit 1 ;; esac",
        );
        let err = iris_service::remove().unwrap_err();
        assert!(
            err.to_string().contains("Failed to connect to bus"),
            "{err}"
        );
        assert!(file.exists(), "a failed stop must keep the file");
    }

    // A failed reload after the stop does not fail the remove: the unit is
    // stopped, disabled and its file is gone.
    fake(&bin, "case \"$*\" in *daemon-reload*) exit 1 ;; esac");
    assert_eq!(iris_service::remove().unwrap(), Some(file.clone()));
    assert!(!file.exists());
    std::fs::remove_dir_all(&dir).unwrap();
}
