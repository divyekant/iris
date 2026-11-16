//! A failed install leaves the service file as it was before. `remove` keeps
//! the service file when the stop fails, so that a second remove can try again. A fake `systemctl` on PATH stands in for systemd.
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

    // A failed install leaves no new unit file, and keeps an old one as it was.
    let unit = home.join(".config/systemd/user/iris.service");
    fake(&bin, "echo 'Failed to connect to bus' >&2; exit 1");
    let err = iris_service::install(Path::new("/opt/iris")).unwrap_err();
    assert!(
        err.to_string().contains("Failed to connect to bus"),
        "{err}"
    );
    assert!(!unit.exists(), "a failed install must remove its new file");
    std::fs::create_dir_all(unit.parent().unwrap()).unwrap();
    std::fs::write(&unit, "old unit").unwrap();
    assert!(iris_service::install(Path::new("/opt/iris")).is_err());
    assert_eq!(std::fs::read_to_string(&unit).unwrap(), "old unit");
    std::fs::remove_file(&unit).unwrap();
    std::fs::remove_file(home.join("calls")).unwrap();

    let file = iris_service::write(
        iris_service::System::Systemd,
        Path::new("/opt/iris"),
        &home,
        &home.join(".config"),
    )
    .unwrap();

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

    fake(&bin, "exit 0");
    assert_eq!(iris_service::remove().unwrap(), Some(file.clone()));
    assert!(!file.exists());
    std::fs::remove_dir_all(&dir).unwrap();
}
