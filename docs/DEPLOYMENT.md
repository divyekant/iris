# Deployment

Iris has no hosted server. Each user runs the `iris` program on their own
computer. A deploy makes the programs of a release and attaches them to the
GitHub Release. Users install a program with `scripts/install.sh`.

## 1. The programs

| Target | System |
|---|---|
| `x86_64-unknown-linux-musl` | Linux, Intel or AMD CPU (static program, any distribution) |
| `aarch64-unknown-linux-musl` | Linux, ARM CPU (for example, a Raspberry Pi 4 or a small ARM server) |
| `aarch64-apple-darwin` | macOS, Apple CPU (M1 or newer) |
| `x86_64-apple-darwin` | macOS, Intel CPU |

Each program is in the archive `iris-<target>.tar.gz`. The file `SHA256SUMS`
has the SHA-256 sum of each archive.

## 2. How a release gets its programs

1. `release.yml` merges the release PR into `main` and publishes the GitHub
   Release (`docs/RELEASES.md`).
2. The `published` event of the release starts `.github/workflows/deploy.yml`.
   The event starts the workflow because `release.py` uses `RELEASE_TOKEN`.
3. `deploy.yml` builds the web client, then the program for each target. It
   builds all targets on Linux with `cargo-zigbuild`. There is no macOS job.
4. `deploy.yml` attaches the archives and `SHA256SUMS` to the release.

The programs come some minutes after the release. In that time,
`install.sh` cannot download the newest release.

If the build of a Linux target fails, the workflow fails and attaches nothing.
If the build of a macOS target fails, the workflow attaches the other programs
and shows a warning. macOS users then build from source (section 5).

To build the programs of a release again, open Actions, select `deploy`,
click Run workflow, and give the tag.

## 3. Secrets

`deploy.yml` uses only the `GITHUB_TOKEN` of the run, with `contents: write`
to attach files to the release. It needs no other secret.

## 4. Install Iris

Use this command on macOS or Linux:

```bash
curl -fsSLO https://raw.githubusercontent.com/divyekant/iris/main/scripts/install.sh && sh install.sh
```

The command saves `install.sh` in the current folder first. If the download
fails, `sh` does not run and the command shows the error.

The script does these steps:

1. It finds the system and the CPU, and selects the archive.
2. It downloads the archive and `SHA256SUMS` from the latest GitHub Release.
3. It checks the SHA-256 sum. If the sum is wrong, it stops and installs
   nothing.
4. It installs the program to `~/.local/bin/iris`. It needs no sudo.
5. It installs and starts the background service with
   `iris service install`. The service needs launchd (macOS) or systemd
   (Linux). If the service install fails (for example, on Linux with no
   systemd), the program stays installed, and the script tells you to start
   `iris serve` yourself. An older program with no `service` command gets
   the same message.

The script connects only to `github.com` (and the GitHub download host that
`github.com` sends it to). It sends no data about you or your computer.

These environment variables change the script:

| Variable | Effect |
|---|---|
| `IRIS_VERSION` | Install this release tag, for example `v2026.11.16.1`, not the latest release. |
| `IRIS_INSTALL_DIR` | Install the program to this folder, not `~/.local/bin`. |
| `IRIS_NO_SERVICE=1` | Do not install the background service. |
| `IRIS_DOWNLOAD_URL` | Download the release files from this folder URL, not from GitHub. The tests use it with a `file://` URL. |

### The background service

The crate `iris-service` writes the service file. The service runs
`iris serve`, which answers on `127.0.0.1:4800`.

| System | Service file | Starts | Output |
|---|---|---|---|
| macOS | `~/Library/LaunchAgents/io.github.divyekant.iris.plist` (launchd agent) | At login | `~/Library/Logs/iris.log` |
| Linux | `~/.config/systemd/user/iris.service` (systemd user unit) | At login | `journalctl --user -u iris` |

Both service managers start the engine again if it stops with an error,
after 10 seconds.

On Linux, a user unit starts only when the user logs in. To start Iris when
the computer starts, with no login (for example, on a small server), turn on
lingering for the user one time:

```bash
loginctl enable-linger "$USER"
```

To stop the service and remove its file, use `iris service remove`.

## 5. Build from source

Use these steps if the release has no program for your system:

1. Install Rust (https://rustup.rs) and Node.js.
2. Clone the repository.
3. Build the client and the program from the root of the repository:

   ```bash
   (cd client && npx --yes pnpm@10.28.0 install --frozen-lockfile && npm run build) && IRIS_REQUIRE_CLIENT=1 cargo build --release
   ```

4. Copy `target/release/iris` to `~/.local/bin/iris`.

## 6. Check a deploy

1. Open the GitHub Release. Make sure that it has the four archives and
   `SHA256SUMS`.
2. Install with `install.sh` on Linux and on the Mac.
3. Make sure that the engine answers:

   ```bash
   curl -fsS http://127.0.0.1:4800/ >/dev/null && echo ok
   ```

4. Make sure that `iris --version` shows the new version.

## 7. Roll back

Install the release before the bad release:

```bash
curl -fsSLO https://raw.githubusercontent.com/divyekant/iris/main/scripts/install.sh && IRIS_VERSION=<old tag> sh install.sh
```

The service file points to `~/.local/bin/iris`, so the service runs the old
program after a restart of the service. To restart it:

- macOS: `launchctl kickstart -k gui/$(id -u)/io.github.divyekant.iris`
- Linux: `systemctl --user restart iris`

To remove a bad release for all users, delete its archives from the GitHub
Release, or mark the release as a pre-release, so that `latest` points to the
release before it.
