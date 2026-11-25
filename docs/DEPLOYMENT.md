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
2. The push to `main` starts `.github/workflows/deploy.yml`. The push starts
   the workflow because `release.py` merges with `RELEASE_TOKEN`. The
   workflow waits up to 15 minutes for the GitHub Release at the pushed
   commit, and uses its tag. With no release, the run fails.
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
The run replaces the files of the same name, then removes the archives that
the new `SHA256SUMS` does not name. If the run stops partway, `install.sh`
refuses each archive whose sum does not match. Run the workflow again to
repair the release.

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

By default, the script connects only to `github.com` and to the GitHub
download host that `github.com` sends it to. These hosts get what each HTTP
request shows: your IP address, the `curl` version, and the archive name,
which names your system and CPU. The script sends no other data. If you set
`IRIS_DOWNLOAD_URL`, the script connects to that address in place of GitHub.

These environment variables change the script:

| Variable | Effect |
|---|---|
| `IRIS_VERSION` | Install this release tag, for example `v2026.11.16.1`, not the latest release. |
| `IRIS_INSTALL_DIR` | Install the program to this folder, not `~/.local/bin`. |
| `IRIS_NO_SERVICE=1` | Do not install the background service. |
| `IRIS_DOWNLOAD_URL` | Download the release files from this folder URL, not from GitHub. The tests use it with a `file://` URL. |

### Agent sessions on Linux

An agent session runs in a sandbox. On Linux, the sandbox needs the program
`bwrap` from the package `bubblewrap`. Install it before you run a job with
an agent:

| System | Command |
|---|---|
| Debian, Ubuntu | `sudo apt install bubblewrap` |
| Fedora, RHEL | `sudo dnf install bubblewrap` |
| Arch Linux | `sudo pacman -S bubblewrap` |

If `bwrap` is not installed, or the system does not permit a user
namespace, `iris job run` stops with the reason, and no agent runs.

### Agent sessions on macOS

On macOS, an agent session runs in a Seatbelt sandbox (`sandbox-exec`) in
a launchd job of its own. No install is necessary. These conditions apply:

- A person must be signed in to the Mac. The launchd job of a session runs
  in the GUI domain of the user (`gui/<uid>`). Over SSH with no signed-in
  person, `iris job run` stops with the reason, and no agent runs.
- A stop has no time limit on macOS. If a process of a session does not
  stop, the session keeps its workspace and its egress port, and the engine
  tries the stop again at its next turn (issue #599).

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

4. Make sure that `iris --version` shows the release tag, for example
   `iris v2026.11.16.1`. `deploy.yml` sets the tag in the program at build
   time (`IRIS_RELEASE`). A build from source shows `iris 0.0.0`.

## 7. Roll back

CAUTION: If the bad release migrated the store, the older program cannot open
the store. It stops with an unknown schema version. The error stays after you
remove an account or the data of a new feature. Only a backup from before the
update lets you roll back.

If you run Iris with another data folder (`--data <path>` or `IRIS_DATA`),
give the same folder to each `iris` command below, for example
`iris --data <path> backup <empty-folder>`. The background service uses
only the default data folder (issue #243).

Before each update, run `iris backup <empty-folder>`. Keep the backup until
you no longer need to roll back. The backup holds your mail and the key, so
keep it private.

To roll back, do these steps:

1. Stop the engine. If the background service runs it, run
   `iris service remove`. Keep the service removed until step 5.
2. Install the release before the bad release, with no service:

   ```bash
   curl -fsSLO https://raw.githubusercontent.com/divyekant/iris/main/scripts/install.sh && IRIS_NO_SERVICE=1 IRIS_VERSION=v2026.11.16.1 sh install.sh
   ```

   Replace `v2026.11.16.1` with the tag of the earlier release (see the
   GitHub Releases page). If the first install set `IRIS_INSTALL_DIR`, set
   the same `IRIS_INSTALL_DIR` in this command. Else the earlier release goes
   to `~/.local/bin`, and `iris` can still run the bad release.

   Run `iris --version`. Make sure that it prints the tag of the earlier
   release. Do not do step 4 before this check: the bad release can migrate
   the restored store again.

   CAUTION: The restore in step 3 removes each local change since the
   backup: decisions, ledger entries, items, drafts and job state. The mail
   syncs again from the mail provider, but these local changes do not come
   back. Keep the moved data folder until you no longer need it.

   CAUTION: `iris backup` copies only the `drafts` folder of each job
   workspace: each regular file of at most 1 MiB, at most 256 files for each
   job. An agent draft that a job wrote after the backup, or a larger draft,
   is only in the `workspaces` folder of the moved data folder. Before you
   start Iris in step 4, copy such a draft from the moved data folder into
   the restored data folder.

3. If the bad release migrated the store, move the data folder away and copy
   the backup in its place. Use the data folder of your installation: the
   path of `--data` or `IRIS_DATA` if you set one. Else it is
   `~/Library/Application Support/iris` on macOS, and `$XDG_DATA_HOME/iris`
   or `~/.local/share/iris` on Linux.
4. Run `iris status` (with the same `--data` if you set one). Make sure that
   it shows your accounts.
5. If you use the default data folder, run `iris service install`. Else
   start `iris serve` with your data folder.

The service file points to `~/.local/bin/iris`, so the service runs the old
program after a restart of the service. To restart it:

- macOS: `launchctl kickstart -k gui/$(id -u)/io.github.divyekant.iris`
- Linux: `systemctl --user restart iris`

To remove a bad release for all users, delete its archives from the GitHub
Release, or mark the release as a pre-release, so that `latest` points to the
release before it.
