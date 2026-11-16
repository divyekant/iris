//! `remove` keeps the service file when the stop fails, so that a second
//! remove can try again. A fake `systemctl` on PATH stands in for systemd.
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
fn a_failed_stop_keeps_the_file_and_a_good_stop_removes_it() {
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
