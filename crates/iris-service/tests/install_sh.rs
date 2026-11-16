//! Tests for `scripts/install.sh`, with release files in a local folder.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const TARGETS: [&str; 4] = [
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
];

fn script() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/install.sh")
}

fn sh(cmd: &str, dir: &Path) {
    let ok = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(dir)
        .status()
        .unwrap()
        .success();
    assert!(ok, "{cmd}");
}

/// A release folder with one archive for each target. The `iris` in each
/// archive is a shell script that records its arguments in `$HOME/calls`.
fn release(dir: &Path) -> PathBuf {
    let rel = dir.join("release");
    let stage = dir.join("stage");
    std::fs::create_dir_all(&rel).unwrap();
    std::fs::create_dir_all(&stage).unwrap();
    std::fs::write(
        stage.join("iris"),
        "#!/bin/sh\necho \"$@\" >> \"$HOME/calls\"\nif [ \"$1\" = service ] && [ -n \"${FAKE_NO_SERVICE:-}\" ]; then echo 'no systemctl' >&2; exit 1; fi\n",
    )
    .unwrap();
    sh("chmod 755 iris", &stage);
    for t in TARGETS {
        sh(&format!("tar -czf ../release/iris-{t}.tar.gz iris"), &stage);
    }
    sh(
        "sha256sum *.tar.gz > SHA256SUMS || shasum -a 256 *.tar.gz > SHA256SUMS",
        &rel,
    );
    rel
}

fn install(dir: &Path, rel: &Path, extra: &[(&str, &str)]) -> Output {
    let home = dir.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let mut cmd = Command::new("sh");
    cmd.arg(script())
        .env("HOME", &home)
        .env("IRIS_DOWNLOAD_URL", format!("file://{}", rel.display()))
        .env_remove("IRIS_INSTALL_DIR")
        .env_remove("IRIS_NO_SERVICE");
    for (k, v) in extra {
        cmd.env(k, v);
    }
    cmd.output().unwrap()
}

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("iris-install-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn installs_to_local_bin_and_installs_the_service() {
    let dir = tmp("ok");
    let rel = release(&dir);
    let out = install(&dir, &rel, &[]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(dir.join("home/.local/bin/iris").is_file());
    let calls = std::fs::read_to_string(dir.join("home/calls")).unwrap();
    assert_eq!(calls, "help service\nservice install\n");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn no_service_skips_the_service() {
    let dir = tmp("noservice");
    let rel = release(&dir);
    let out = install(&dir, &rel, &[("IRIS_NO_SERVICE", "1")]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(dir.join("home/.local/bin/iris").is_file());
    assert!(!dir.join("home/calls").exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_wrong_sum_installs_nothing() {
    let dir = tmp("badsum");
    let rel = release(&dir);
    let sums = std::fs::read_to_string(rel.join("SHA256SUMS")).unwrap();
    let bad: String = sums
        .lines()
        .map(|l| format!("{}{}\n", "0".repeat(64), &l[64..]))
        .collect();
    std::fs::write(rel.join("SHA256SUMS"), bad).unwrap();
    let out = install(&dir, &rel, &[]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("SHA-256 sum"));
    assert!(!dir.join("home/.local/bin/iris").exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_missing_sum_line_installs_nothing() {
    let dir = tmp("nosum");
    let rel = release(&dir);
    std::fs::write(rel.join("SHA256SUMS"), "").unwrap();
    let out = install(
        &dir,
        &rel,
        &[("IRIS_INSTALL_DIR", dir.join("bin").to_str().unwrap())],
    );
    assert!(!out.status.success());
    assert!(!dir.join("bin/iris").exists());
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn a_system_with_no_service_manager_keeps_the_program() {
    let dir = tmp("nomanager");
    let rel = release(&dir);
    let out = install(&dir, &rel, &[("FAKE_NO_SERVICE", "1")]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(dir.join("home/.local/bin/iris").is_file());
    assert!(stderr.contains("the background service is not"), "{stderr}");
    assert!(stderr.contains("iris serve"), "{stderr}");
    std::fs::remove_dir_all(&dir).unwrap();
}
