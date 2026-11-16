//! Install the real `iris` program with `scripts/install.sh` from a local
//! release folder, and check that the installer starts the background
//! service. A fake `systemctl` and `launchctl` on PATH stand in for the
//! service managers.

use std::path::{Path, PathBuf};
use std::process::Command;

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

#[test]
fn install_sh_installs_the_real_program_and_its_service() {
    let dir = std::env::temp_dir().join(format!("iris-install-real-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let (stage, rel, home, bin) = (
        dir.join("stage"),
        dir.join("release"),
        dir.join("home"),
        dir.join("bin"),
    );
    for d in [&stage, &rel, &home, &bin] {
        std::fs::create_dir_all(d).unwrap();
    }
    std::fs::copy(env!("CARGO_BIN_EXE_iris"), stage.join("iris")).unwrap();
    for t in [
        "x86_64-unknown-linux-musl",
        "aarch64-unknown-linux-musl",
        "x86_64-apple-darwin",
        "aarch64-apple-darwin",
    ] {
        sh(&format!("tar -czf ../release/iris-{t}.tar.gz iris"), &stage);
    }
    sh(
        "sha256sum *.tar.gz > SHA256SUMS || shasum -a 256 *.tar.gz > SHA256SUMS",
        &rel,
    );
    // `launchctl print` exits 113 ("Could not find service"): no agent is loaded yet.
    for tool in ["systemctl", "launchctl"] {
        std::fs::write(
            bin.join(tool),
            format!("#!/bin/sh\necho \"{tool} $*\" >> \"$HOME/calls\"\n[ \"$1\" != print ] || exit 113\n"),
        )
        .unwrap();
    }
    sh("chmod 755 systemctl launchctl", &bin);

    let script: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/install.sh");
    let out = Command::new("sh")
        .arg(script)
        .env("HOME", &home)
        .env(
            "PATH",
            format!(
                "{}:{}",
                bin.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .env("IRIS_DOWNLOAD_URL", format!("file://{}", rel.display()))
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("IRIS_INSTALL_DIR")
        .env_remove("IRIS_NO_SERVICE")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        stdout.contains("installed and started the service"),
        "{stdout}"
    );

    let program = home.join(".local/bin/iris").canonicalize().unwrap();
    let calls = std::fs::read_to_string(home.join("calls")).unwrap();
    let file = if cfg!(target_os = "macos") {
        assert!(calls.contains("launchctl bootstrap gui/"), "{calls}");
        home.join("Library/LaunchAgents/io.github.divyekant.iris.plist")
    } else {
        assert_eq!(calls, "systemctl --user daemon-reload\nsystemctl --user enable iris.service\nsystemctl --user restart iris.service\n");
        home.join(".config/systemd/user/iris.service")
    };
    let text = std::fs::read_to_string(file).unwrap();
    assert!(text.contains(program.to_str().unwrap()), "{text}");
    assert!(text.contains("serve"));
    std::fs::remove_dir_all(&dir).unwrap();
}
