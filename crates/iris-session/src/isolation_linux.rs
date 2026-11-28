//! The Linux sandbox of a session (`docs/specs/2026-11-17-session-sandbox.md`,
//! "Linux"): bubblewrap (`bwrap`) with new namespaces, a file view of the
//! system folders and the workspace, and one relay from the network
//! namespace of the session to its egress port.

use std::ffi::{CString, OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, Shutdown, TcpListener, TcpStream};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use iris_job::format_session_id;

use super::{Command, Isolation};
use crate::egress::{send_fd, start_handoff, AfterEnd};

/// The file descriptor on which `bwrap` writes its info line.
const INFO_FD: RawFd = 3;
/// The file descriptor on which `bwrap` waits before it runs the program.
const BLOCK_FD: RawFd = 4;
/// The time limit of the setup in `spawn` (the info line, the record and
/// the listener), inside the 2-second start gate of the launcher.
const SETUP_WAIT: Duration = Duration::from_millis(1500);
/// The time limit of the probe of `check`.
const PROBE_WAIT: Duration = Duration::from_secs(5);
/// The longest info line that `spawn` reads.
const INFO_MAX: usize = 4096;
/// The longest wait of `stop` for the end of the namespace.
const STOP_WAIT: Duration = Duration::from_secs(5);
/// The first file descriptor of a read path in `bwrap` (`--ro-bind-fd`).
/// The helper puts the open read paths there, in order.
const READ_FD: RawFd = 5;
/// The option of `bwrap` that binds the object of a descriptor.
const RO_BIND_FD: &str = "--ro-bind-fd";
/// The most open connections of the relay, as in the egress proxy.
pub(crate) const RELAY_MAX: usize = 8;
/// The system files of `/etc` that programs read at the start, shown
/// read-only if present. Each other file of `/etc` stays out of the view.
const ETC: &[&str] = &[
    "/etc/ld.so.cache",
    "/etc/ld.so.conf",
    "/etc/ld.so.conf.d",
    "/etc/passwd",
    "/etc/group",
    "/etc/nsswitch.conf",
    "/etc/hosts",
    "/etc/localtime",
    "/etc/os-release",
    // Only the public certificate authority files: `/etc/ssl/private` and
    // `/etc/pki/tls/private` hold private keys.
    "/etc/ssl/certs",
    "/etc/ssl/cert.pem",
    "/etc/ssl/openssl.cnf",
    "/etc/pki/tls/certs",
    "/etc/pki/tls/cert.pem",
    "/etc/pki/tls/openssl.cnf",
    "/etc/pki/ca-trust/extracted",
    "/etc/ca-certificates/extracted",
    "/etc/alternatives",
];
/// The private home of a session when the engine has no usable `HOME`.
const PRIVATE_HOME: &str = "/home/iris";
/// The system folders that are a link to `/usr` on most systems.
const SYSTEM_LINKS: &[&str] = &["/bin", "/sbin", "/lib", "/lib32", "/lib64", "/libx32"];

/// The Linux isolation.
pub struct Bwrap {
    bwrap: PathBuf,
    data_dir: PathBuf,
    sessions: PathBuf,
}

impl Bwrap {
    /// Finds `bwrap`: `/usr/bin/bwrap`, then the absolute folders of `PATH`.
    pub fn new(data_dir: &Path) -> Result<Self, String> {
        let bwrap = find_bwrap(std::env::var_os("PATH").as_deref()).ok_or_else(|| {
            "bubblewrap (bwrap) is not installed: install the package bubblewrap".to_string()
        })?;
        Self::with(bwrap, data_dir)
    }

    /// The data folder is resolved first: `bwrap` mounts at the path as it
    /// is given, so a link would leave the real folder in the view.
    pub(crate) fn with(bwrap: PathBuf, data_dir: &Path) -> Result<Self, String> {
        let data_dir = fs::canonicalize(data_dir).map_err(|e| {
            format!(
                "the data folder {} cannot be resolved: {e}",
                data_dir.display()
            )
        })?;
        Ok(Self {
            bwrap,
            sessions: data_dir.join("sessions"),
            data_dir,
        })
    }

    fn record_path(&self, session: i64) -> PathBuf {
        self.sessions
            .join(format!("{}.sandbox", format_session_id(session)))
    }

    /// The options of the namespaces and of the file view. `home` gets an
    /// empty `tmpfs`; `reads` are extra read-only paths (the program
    /// files); `bound` are the read paths of the adapter, which the helper
    /// binds by descriptor ([`READ_FD`] on).
    fn view(
        &self,
        home: Option<&Path>,
        reads: &[PathBuf],
        bound: &[PathBuf],
        workspace: &Path,
    ) -> Vec<OsString> {
        let mut a: Vec<OsString> = Vec::new();
        let mut push = |items: &[&OsStr]| a.extend(items.iter().map(|s| s.to_os_string()));
        push(&[
            "--unshare-all".as_ref(),
            "--die-with-parent".as_ref(),
            "--new-session".as_ref(),
        ]);
        // /usr without /usr/local: local programs and settings stay out.
        push(&[
            "--ro-bind".as_ref(),
            "/usr".as_ref(),
            "/usr".as_ref(),
            "--tmpfs".as_ref(),
            "/usr/local".as_ref(),
        ]);
        for path in ETC {
            push(&["--ro-bind-try".as_ref(), path.as_ref(), path.as_ref()]);
        }
        for link in SYSTEM_LINKS {
            let p = Path::new(link);
            match fs::symlink_metadata(p) {
                Ok(m) if m.file_type().is_symlink() => {
                    if let Ok(target) = fs::read_link(p) {
                        push(&["--symlink".as_ref(), target.as_os_str(), p.as_os_str()]);
                    }
                }
                Ok(m) if m.is_dir() => push(&["--ro-bind".as_ref(), p.as_os_str(), p.as_os_str()]),
                _ => {}
            }
        }
        push(&[
            "--proc".as_ref(),
            "/proc".as_ref(),
            "--dev".as_ref(),
            "/dev".as_ref(),
            "--tmpfs".as_ref(),
            "/tmp".as_ref(),
        ]);
        // Each path that the agent gets for a place to write is in the
        // view: the private home at its real path (bwrap does not resolve a
        // link in a mount point), or PRIVATE_HOME if HOME is absent or not
        // usable, and the private /tmp.
        let home = home
            .map(|h| fs::canonicalize(h).unwrap_or_else(|_| h.to_path_buf()))
            .filter(|h| h.is_absolute() && h.parent().is_some())
            .unwrap_or_else(|| PathBuf::from(PRIVATE_HOME));
        push(&[
            "--tmpfs".as_ref(),
            home.as_os_str(),
            "--setenv".as_ref(),
            "HOME".as_ref(),
            home.as_os_str(),
            "--setenv".as_ref(),
            "TMPDIR".as_ref(),
            "/tmp".as_ref(),
        ]);
        for r in reads {
            push(&["--ro-bind".as_ref(), r.as_os_str(), r.as_os_str()]);
        }
        for (i, r) in bound.iter().enumerate() {
            let fd = OsString::from((READ_FD + i as RawFd).to_string());
            push(&[RO_BIND_FD.as_ref(), fd.as_os_str(), r.as_os_str()]);
        }
        // The data folder is empty in the view, also when it is inside a
        // system folder or a read path. Then the workspace comes back.
        push(&[
            "--tmpfs".as_ref(),
            self.data_dir.as_os_str(),
            "--bind".as_ref(),
            workspace.as_os_str(),
            workspace.as_os_str(),
            // The root of the view, /usr/local and the data folder are
            // tmpfs mounts: no write there. A remount is not recursive, so
            // the workspace and the program files keep their own mounts.
            "--remount-ro".as_ref(),
            "/usr/local".as_ref(),
            "--remount-ro".as_ref(),
            self.data_dir.as_os_str(),
            "--remount-ro".as_ref(),
            "/".as_ref(),
            "--chdir".as_ref(),
            workspace.as_os_str(),
        ]);
        a
    }

    /// The probe of [`Isolation::check`]: `true` in the full sandbox.
    fn probe(&self) -> Result<(), String> {
        // The data folder in the place of a workspace: each mount of a real
        // view happens. The probe runs only `true`.
        let mut args = self.view(None, &[], &[], &self.data_dir);
        args.push("--".into());
        args.push("true".into());
        let mut child = std::process::Command::new(&self.bwrap)
            .args(&args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("bwrap does not start: {e}"))?;
        let start = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(s)) if s.success() => return Ok(()),
                Ok(Some(s)) => {
                    let mut err = String::new();
                    if let Some(mut e) = child.stderr.take() {
                        let _ = e.by_ref().take(512).read_to_string(&mut err);
                    }
                    return Err(format!("bwrap cannot make a sandbox ({s}): {}", err.trim()));
                }
                Ok(None) if start.elapsed() < PROBE_WAIT => {
                    thread::sleep(Duration::from_millis(10))
                }
                _ => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("bwrap did not end in 5 seconds".into());
                }
            }
        }
    }

    /// [`Isolation::spawn`] with the egress port of the session and the
    /// helper end of the socket pair of its proxy.
    pub(crate) fn spawn_with(
        &self,
        session: i64,
        command: &mut std::process::Command,
        egress: Option<(u16, OwnedFd)>,
    ) -> io::Result<Child> {
        // The read paths of the command: each one is opened and checked
        // again here, just before bwrap starts, and bwrap binds the object
        // of the descriptor, not a path (read paths spec, "Linux").
        let opened = self.open_reads(command)?;
        let local_port = local_port_of(command);
        let read_fds: Vec<RawFd> = opened.iter().map(|(f, _)| f.as_raw_fd()).collect();
        // Each folder of a read path goes into the view through a
        // read-only overlay (issue 996). Made before the fork: the child
        // only runs system calls on it.
        let mut overlays = Overlays::plan(&opened)?;
        let has_overlays = overlays.is_some();
        let (info_r, info_w) = pipe()?;
        let (block_r, block_w) = pipe()?;
        let (iw, br) = (info_w.as_raw_fd(), block_r.as_raw_fd());
        // SAFETY: the closure runs in the child between fork and exec. It
        // calls only fcntl and dup2, which are async-signal-safe, and reads
        // `read_fds` with no allocation. dup2 clears FD_CLOEXEC on the new
        // descriptor, so bwrap gets fds 3, 4 and the read paths.
        unsafe {
            command.pre_exec(move || {
                // Move each source out of the target range first (3 to
                // 4 + the read paths), so one dup2 cannot close another.
                let iw2 = libc::fcntl(iw, libc::F_DUPFD_CLOEXEC, 64);
                let br2 = libc::fcntl(br, libc::F_DUPFD_CLOEXEC, 64);
                let mut high = [0 as RawFd; crate::read_paths::MAX_READS];
                for (i, fd) in read_fds.iter().enumerate() {
                    high[i] = libc::fcntl(*fd, libc::F_DUPFD_CLOEXEC, 64);
                    if high[i] < 0 {
                        return Err(io::Error::last_os_error());
                    }
                }
                if iw2 < 0
                    || br2 < 0
                    || libc::dup2(iw2, INFO_FD) < 0
                    || libc::dup2(br2, BLOCK_FD) < 0
                {
                    return Err(io::Error::last_os_error());
                }
                // Each read path to its number.
                for (i, fd) in high.iter().take(read_fds.len()).enumerate() {
                    if libc::dup2(*fd, READ_FD + i as RawFd) < 0 {
                        return Err(io::Error::last_os_error());
                    }
                }
                if let Some(o) = &mut overlays {
                    o.apply()?;
                }
                Ok(())
            });
        }
        // The engine: the relay of the local port connects only while it
        // runs.
        // SAFETY: getppid has no memory effects.
        let engine = pidfd_open(unsafe { libc::getppid() })?;
        let mut child = command.spawn().map_err(|e| {
            if has_overlays {
                // Names the cause for the person (read paths spec, "Linux").
                io::Error::new(
                    e.kind(),
                    format!(
                        "the read-only overlay of a read path folder failed ({e}): \
                         the system must permit an overlay in a user namespace \
                         for the iris program"
                    ),
                )
            } else {
                e
            }
        })?;
        drop(info_w);
        drop(block_r);
        let mut init: Option<OwnedFd> = None;
        let result = (|| -> io::Result<()> {
            let deadline = Instant::now() + SETUP_WAIT;
            let pid = read_info(info_r, deadline)?;
            let (fd, info) =
                identify(pid)?.ok_or_else(|| io::Error::other("the init of the sandbox exited"))?;
            init = Some(fd);
            self.write_record(session, &info)?;
            if let Some((port, channel)) = egress {
                let listener = listen_in(info.pid, port, deadline)?;
                start_handoff(listener, channel, AfterEnd::Close)?;
            }
            if let Some(port) = local_port {
                // The server on the port is a program of the person, not
                // an Iris proxy: the relay connects from 127.0.0.1.
                let listener = listen_in(info.pid, port, deadline)?;
                start_relay(listener, port, Ipv4Addr::LOCALHOST, engine)?;
            }
            // bwrap runs the program of the adapter after this byte.
            (&File::from(block_w)).write_all(b"x")
        })();
        if let Err(e) = result {
            if let Some(fd) = init {
                let _ = kill_and_wait(&fd);
            }
            // SAFETY: kill has no memory effects. The child is not reaped
            // yet, so its process ID is its own.
            unsafe {
                libc::kill(child.id() as i32, libc::SIGKILL);
            }
            let _ = child.wait();
            return Err(e);
        }
        Ok(child)
    }

    /// Opens and checks each read path of `command` (the pairs
    /// `--ro-bind-fd <n> <path>`, in order from [`READ_FD`]). The helper
    /// runs the checks of the launch again on the object that it binds,
    /// with no link in the path: a change after the launch fails here.
    fn open_reads(&self, command: &std::process::Command) -> io::Result<Vec<(OwnedFd, PathBuf)>> {
        let args: Vec<&OsStr> = command.get_args().collect();
        let home = std::env::var_os("HOME");
        let mut out = Vec::new();
        let mut i = 0;
        while i < args.len() {
            if args[i] == RO_BIND_FD {
                let (Some(n), Some(path)) = (args.get(i + 1), args.get(i + 2)) else {
                    return Err(io::Error::other("a read path option has no path"));
                };
                if out.len() >= crate::read_paths::MAX_READS
                    || n.to_str() != Some(&(READ_FD + out.len() as RawFd).to_string())
                {
                    return Err(io::Error::other("the read path options are out of order"));
                }
                let path = Path::new(path);
                let c = crate::read_paths::check(path, &self.data_dir, home.as_deref())
                    .map_err(io::Error::other)?;
                if c.real != path {
                    return Err(io::Error::other(format!(
                        "the read path {} changed after the launch",
                        path.display()
                    )));
                }
                out.push((c.fd, c.real));
                i += 3;
            } else if args[i] == "--" {
                break;
            } else {
                i += 1;
            }
        }
        Ok(out)
    }

    fn write_record(&self, session: i64, info: &Info) -> io::Result<()> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.sessions)?;
        // Complete or absent after a crash: write and sync a temporary
        // file, then link it to the name (link fails if the name exists).
        let record = self.record_path(session);
        let mut tmp = record.clone().into_os_string();
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        writeln!(
            f,
            "{} {} {} {}",
            info.boot, info.pid, info.start, info.pid_ns
        )?;
        f.sync_all()?;
        fs::hard_link(&tmp, &record)?;
        File::open(&self.sessions)?.sync_all()?;
        let _ = fs::remove_file(&tmp);
        Ok(())
    }

    /// The record of the session: the identity of the init. `None` if
    /// there is no record.
    fn record(&self, session: i64) -> io::Result<Option<Info>> {
        let text = match fs::read_to_string(self.record_path(session)) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e),
        };
        let p: Vec<&str> = text.split_whitespace().collect();
        let info = match p[..] {
            [boot, pid, start, ns] => (|| {
                Some(Info {
                    boot: boot.to_string(),
                    pid: pid.parse().ok().filter(|p| *p > 1)?,
                    start: start.parse().ok()?,
                    pid_ns: ns.parse().ok()?,
                })
            })(),
            _ => None,
        };
        info.map(Some)
            .ok_or_else(|| io::Error::other("the sandbox record is not valid"))
    }

    /// A `pidfd` of the init of the record, if the init still runs (not a
    /// zombie). `None`: the init of the record ended, so its namespace is
    /// empty.
    fn open_init(&self, rec: &Info) -> io::Result<Option<OwnedFd>> {
        if boot_id()? != rec.boot {
            return Ok(None); // the system started again
        }
        Ok(identify(rec.pid)?.and_then(|(fd, now)| (now == *rec).then_some(fd)))
    }
}

impl Isolation for Bwrap {
    fn check(&self) -> Result<(), String> {
        // pidfd_open (Linux 5.3) on this process.
        let fd = pidfd_open(std::process::id() as i32)
            .map_err(|e| format!("this kernel has no pidfd_open: {e}"))?;
        drop(fd);
        self.probe()
    }

    /// `bwrap` with the view of the spec. The egress port is not in the
    /// command: the helper reads it from its own `HTTPS_PROXY`, which the
    /// launch sets only for an adapter with allowed hosts.
    fn command(
        &self,
        _session: i64,
        workspace: &Path,
        _egress_port: Option<u16>,
        program: &Path,
        args: &[String],
        bound: &[PathBuf],
        local_port: Option<u16>,
    ) -> Command {
        // The real path of the program, so that a link in the home of the
        // person is not necessary.
        let program = fs::canonicalize(program).unwrap_or_else(|_| program.to_path_buf());
        // The file only: its folder can hold files of the person.
        let mut reads: Vec<PathBuf> = vec![program.clone()];
        // The iris program, for the verbs of the agent on its socket.
        let iris = std::env::current_exe().and_then(fs::canonicalize).ok();
        if let Some(exe) = &iris {
            reads.push(exe.clone());
        }
        // Each mount point of the view is a real path: bwrap does not
        // resolve a link in a destination.
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let workspace = fs::canonicalize(workspace).unwrap_or_else(|_| workspace.to_path_buf());
        let mut a = self.view(home.as_deref(), &reads, bound, &workspace);
        // The helper reads the local port from this pair, and the agent
        // learns it too.
        if let Some(port) = local_port {
            a.extend([
                "--setenv".into(),
                crate::LOCAL_PORT_VAR.into(),
                port.to_string().into(),
            ]);
        }
        if let Some(dir) = iris.as_deref().and_then(Path::parent) {
            let mut path = dir.as_os_str().to_os_string();
            if let Some(p) = std::env::var_os("PATH") {
                path.push(":");
                path.push(p);
            }
            a.extend(["--setenv".into(), "PATH".into(), path]);
        }
        a.extend([
            "--info-fd".into(),
            INFO_FD.to_string().into(),
            "--block-fd".into(),
            BLOCK_FD.to_string().into(),
            "--".into(),
            program.into_os_string(),
        ]);
        a.extend(args.iter().map(OsString::from));
        Command {
            program: self.bwrap.clone(),
            args: a,
        }
    }

    fn name(&self, session: i64) -> String {
        format!("bwrap:{}", self.record_path(session).display())
    }

    fn stop(&self, session: i64) -> io::Result<()> {
        let Some(rec) = self.record(session)? else {
            return Ok(());
        };
        match self.open_init(&rec)? {
            Some(fd) => kill_and_wait(&fd),
            None => Ok(()),
        }
    }

    fn ended(&self, session: i64) -> io::Result<bool> {
        let Some(rec) = self.record(session)? else {
            return Ok(true);
        };
        Ok(self.open_init(&rec)?.is_none())
    }

    fn spawn(&self, session: i64, command: &mut std::process::Command) -> io::Result<Child> {
        let port = std::env::var("HTTPS_PROXY")
            .ok()
            .and_then(|v| v.strip_prefix("http://127.0.0.1:")?.parse::<u16>().ok());
        let egress = match port {
            Some(port) => Some((port, take_channel()?)),
            None => None,
        };
        self.spawn_with(session, command, egress)
    }
}

/// Room for the `lowerdir` value of one overlay,
/// `/proc/self/fd/<n>:/proc/self/fd/<n>`, and its zero byte.
const LOWER_LEN: usize = 64;

/// The overlays of the read path folders (issue 996). A program of the
/// person can make a socket in a read path after the checks. The kernel
/// finds a socket by the inode of its file, and an overlay shows each file
/// with an inode of its own, so a `connect` through the overlay is refused
/// (`ECONNREFUSED`). The child of the spawn makes a new user and mount
/// namespace, puts an overlay on each folder there, and gives `bwrap` the
/// overlay in the place of the folder. `bwrap` runs in that namespace.
/// A read path that is a file needs no overlay: no socket can appear in it.
struct Overlays {
    /// The lines of `/proc/self/{setgroups,uid_map,gid_map}`.
    maps: [(CString, CString); 3],
    /// For each folder: its read path number, its path, and its device
    /// and inode.
    /// The outer folders come first.
    dirs: Vec<(usize, CString, (u64, u64))>,
    /// For each folder that another read path folder holds: the index in
    /// `dirs` of the nearest such folder, and the path from it.
    inner: Vec<Option<(usize, CString)>>,
    /// Test only: after the identity checks of `apply` and before the
    /// overlays, move the folder `.0` to `.1` and put a link to `.2` at `.0`.
    #[cfg(test)]
    swap: Option<(CString, CString, CString)>,
    /// For each folder, written in the child with no allocation: its
    /// descriptor in the new namespace, and the `lowerdir` value.
    fds: Vec<RawFd>,
    lower: Vec<[u8; LOWER_LEN]>,
}

impl Overlays {
    fn plan(opened: &[(OwnedFd, PathBuf)]) -> io::Result<Option<Overlays>> {
        let mut found = Vec::new();
        for (i, (fd, path)) in opened.iter().enumerate() {
            let m = File::from(fd.try_clone()?).metadata()?;
            if m.is_dir() {
                found.push((i, path, (m.dev(), m.ino())));
            }
        }
        if found.is_empty() {
            return Ok(None);
        }
        found.sort_by_key(|(_, path, _)| path.components().count());
        let mut inner = Vec::new();
        for (_, p, _) in &found {
            // The outer folders come first, so the last one that holds `p`
            // is the nearest.
            let outer = found
                .iter()
                .enumerate()
                .rfind(|(_, (_, o, _))| o != p && p.starts_with(o));
            inner.push(match outer {
                Some((o, (_, op, _))) => {
                    let rel = p.strip_prefix(op).map_err(io::Error::other)?;
                    Some((o, cstr(rel.as_os_str())?))
                }
                None => None,
            });
        }
        let dirs = found
            .into_iter()
            .map(|(i, path, id)| Ok((i, cstr(path.as_os_str())?, id)))
            .collect::<io::Result<Vec<_>>>()?;
        let c = |s: &str| CString::new(s).map_err(io::Error::other);
        // SAFETY: getuid and getgid have no memory effects.
        let (uid, gid) = unsafe { (libc::getuid(), libc::getgid()) };
        Ok(Some(Overlays {
            maps: [
                (c("/proc/self/setgroups")?, c("deny")?),
                (c("/proc/self/uid_map")?, c(&format!("{uid} {uid} 1"))?),
                (c("/proc/self/gid_map")?, c(&format!("{gid} {gid} 1"))?),
            ],
            fds: vec![-1; dirs.len()],
            lower: vec![[0; LOWER_LEN]; dirs.len()],
            dirs,
            inner,
            #[cfg(test)]
            swap: None,
        }))
    }

    /// Runs in the child of the spawn, after the read paths are at their
    /// numbers. Only system calls, no allocation. Each new descriptor is
    /// one that the kernel gives, so no fixed number can close another.
    ///
    /// # Safety
    ///
    /// Call only in the child of a fork, before exec.
    unsafe fn apply(&mut self) -> io::Result<()> {
        let err = || Err(io::Error::last_os_error());
        if libc::unshare(libc::CLONE_NEWUSER | libc::CLONE_NEWNS) != 0 {
            return err();
        }
        for (path, line) in &self.maps {
            let fd = libc::open(path.as_ptr(), libc::O_WRONLY | libc::O_CLOEXEC);
            if fd < 0 {
                return err();
            }
            let n = line.as_bytes().len();
            let w = libc::write(fd, line.as_ptr() as *const libc::c_void, n);
            libc::close(fd);
            if w != n as isize {
                return err();
            }
        }
        // No mount of this namespace goes back to the host.
        if libc::mount(
            std::ptr::null(),
            c"/".as_ptr(),
            std::ptr::null(),
            libc::MS_REC | libc::MS_PRIVATE,
            std::ptr::null(),
        ) != 0
        {
            return err();
        }
        // Each folder again, at its path in this namespace, before any
        // overlay: an overlay on one folder changes the inodes of each
        // folder in it. It must be the folder that the checks opened (a
        // folder has one path: no hard link).
        for (k, (_, path, id)) in self.dirs.iter().enumerate() {
            let lo = libc::open(
                path.as_ptr(),
                libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC,
            );
            if lo < 0 {
                return err();
            }
            self.fds[k] = lo;
            let mut st: libc::stat = std::mem::zeroed();
            if libc::fstat(lo, &mut st) != 0 {
                return err();
            }
            if (st.st_dev as u64, st.st_ino as u64) != *id {
                return Err(io::Error::from_raw_os_error(libc::ESTALE));
            }
        }
        #[cfg(test)]
        if let Some((from, to, link)) = &self.swap {
            if libc::rename(from.as_ptr(), to.as_ptr()) != 0
                || libc::symlink(link.as_ptr(), from.as_ptr()) != 0
            {
                return err();
            }
        }
        let empty = fs_mount(c"tmpfs", None)?;
        for k in 0..self.dirs.len() {
            let (lo, slot) = (self.fds[k], READ_FD + self.dirs[k].0 as RawFd);
            let lower = lower_value(&mut self.lower[k], lo, empty);
            let m = fs_mount(c"overlay", Some(lower))?;
            // Put the overlay on the folder in this namespace, then give
            // its root to bwrap in the place of the folder. The slot holds
            // the checked folder, so the kernel never gives its number.
            // bwrap 0.11 binds the path of the descriptor, not the object,
            // and refuses a mount that is not at the path. So an inner
            // folder goes on its path in the overlay of its outer folder,
            // which is on the path now (the outer folders come first). The
            // open stays in that overlay: no link, no other mount. A folder
            // that took the path after the checks is in the view of the
            // outer folder already, so the inner overlay shows no more.
            let target = match &self.inner[k] {
                Some((o, rel)) => {
                    let mut how: libc::open_how = std::mem::zeroed();
                    how.flags = (libc::O_PATH | libc::O_DIRECTORY | libc::O_CLOEXEC) as u64;
                    how.resolve = libc::RESOLVE_BENEATH
                        | libc::RESOLVE_NO_SYMLINKS
                        | libc::RESOLVE_NO_MAGICLINKS
                        | libc::RESOLVE_NO_XDEV;
                    libc::syscall(
                        libc::SYS_openat2,
                        READ_FD + self.dirs[*o].0 as RawFd,
                        rel.as_ptr(),
                        &how as *const libc::open_how,
                        std::mem::size_of::<libc::open_how>(),
                    ) as RawFd
                }
                None => lo,
            };
            if target < 0 {
                return err();
            }
            let moved = libc::syscall(
                libc::SYS_move_mount,
                m,
                c"".as_ptr(),
                target,
                c"".as_ptr(),
                MOVE_MOUNT_F_EMPTY_PATH | MOVE_MOUNT_T_EMPTY_PATH,
            );
            if target != lo {
                libc::close(target);
            }
            if moved != 0 || libc::dup2(m, slot) < 0 {
                return err();
            }
            libc::close(m);
            libc::close(lo);
        }
        libc::close(empty);
        Ok(())
    }
}

/// Writes `/proc/self/fd/<lower>:/proc/self/fd/<empty>` and a zero byte
/// into `buf`, with no allocation, and gives it.
fn lower_value(buf: &mut [u8; LOWER_LEN], lower: RawFd, empty: RawFd) -> &std::ffi::CStr {
    let mut n = 0;
    let mut put = |b: &[u8]| {
        buf[n..n + b.len()].copy_from_slice(b);
        n += b.len();
    };
    for (k, fd) in [lower, empty].into_iter().enumerate() {
        put(if k == 0 {
            b"/proc/self/fd/"
        } else {
            b":/proc/self/fd/"
        });
        let (mut digits, mut d, mut v) = ([0u8; 10], 10, fd as u32);
        loop {
            d -= 1;
            digits[d] = b'0' + (v % 10) as u8;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        put(&digits[d..]);
    }
    put(b"\0");
    std::ffi::CStr::from_bytes_until_nul(buf).expect("the value has its zero byte")
}

const FSCONFIG_SET_STRING: libc::c_uint = 1;
const FSCONFIG_CMD_CREATE: libc::c_uint = 6;
const MOUNT_ATTR_RDONLY: libc::c_uint = 0x1;
const MOUNT_ATTR_NOSUID: libc::c_uint = 0x2;
const MOUNT_ATTR_NODEV: libc::c_uint = 0x4;
const MOVE_MOUNT_F_EMPTY_PATH: libc::c_uint = 0x4;
const MOVE_MOUNT_T_EMPTY_PATH: libc::c_uint = 0x40;

/// A new detached read-only mount of the file system `kind`, with the
/// option `lowerdir` if given (new mount API, Linux 5.2).
///
/// # Safety
///
/// Only system calls: safe in the child of a fork.
unsafe fn fs_mount(kind: &std::ffi::CStr, lowerdir: Option<&std::ffi::CStr>) -> io::Result<RawFd> {
    let fs = libc::syscall(libc::SYS_fsopen, kind.as_ptr(), 1 as libc::c_uint) as RawFd;
    if fs < 0 {
        return Err(io::Error::last_os_error());
    }
    let ok = lowerdir.is_none_or(|l| {
        libc::syscall(
            libc::SYS_fsconfig,
            fs,
            FSCONFIG_SET_STRING,
            c"lowerdir".as_ptr(),
            l.as_ptr(),
            0,
        ) == 0
    }) && libc::syscall(
        libc::SYS_fsconfig,
        fs,
        FSCONFIG_CMD_CREATE,
        std::ptr::null::<libc::c_char>(),
        std::ptr::null::<libc::c_void>(),
        0,
    ) == 0;
    let m = if ok {
        libc::syscall(
            libc::SYS_fsmount,
            fs,
            1 as libc::c_uint,
            MOUNT_ATTR_RDONLY | MOUNT_ATTR_NOSUID | MOUNT_ATTR_NODEV,
        ) as RawFd
    } else {
        -1
    };
    let e = io::Error::last_os_error();
    libc::close(fs);
    if m < 0 {
        return Err(e);
    }
    Ok(m)
}

fn cstr(s: &OsStr) -> io::Result<CString> {
    use std::os::unix::ffi::OsStrExt;
    CString::new(s.as_bytes()).map_err(io::Error::other)
}

/// The local port of `command`: the value of the pair
/// `--setenv IRIS_LOCAL_PORT <port>` that [`Isolation::command`] writes.
fn local_port_of(command: &std::process::Command) -> Option<u16> {
    // Only the options of bwrap: the arguments after `--` are the
    // program's.
    let args: Vec<&OsStr> = command.get_args().take_while(|a| *a != "--").collect();
    args.windows(3)
        .find(|w| w[0] == "--setenv" && w[1] == crate::LOCAL_PORT_VAR)
        .and_then(|w| w[2].to_str()?.parse().ok())
}

fn find_bwrap(path: Option<&OsStr>) -> Option<PathBuf> {
    let fixed = PathBuf::from("/usr/bin/bwrap");
    if fixed.is_file() {
        return Some(fixed);
    }
    std::env::split_paths(path?)
        .filter(|d| d.is_absolute())
        .map(|d| d.join("bwrap"))
        .find(|p| p.is_file())
}

/// The identity of the init of a sandbox: the boot, the host process ID,
/// the start time (clock ticks after the boot) and the inode of its PID
/// namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Info {
    pub boot: String,
    pub pid: i32,
    pub start: u64,
    pub pid_ns: u64,
}

fn boot_id() -> io::Result<String> {
    Ok(fs::read_to_string("/proc/sys/kernel/random/boot_id")?
        .trim()
        .to_string())
}

/// The identity of the running process `pid`, with a `pidfd` of it.
/// `None` if no process has the ID, or if it exited (a zombie). The
/// `pidfd` holds the process: if it still runs after the reads of `/proc`,
/// the ID was its own during the reads (an ID goes to another process only
/// after the reap), so the values are its own.
pub(crate) fn identify(pid: i32) -> io::Result<Option<(OwnedFd, Info)>> {
    let fd = match pidfd_open(pid) {
        Ok(fd) => fd,
        Err(e) if exited(&e) => return Ok(None),
        Err(e) => return Err(e),
    };
    // A read of `/proc` fails in more than one way when the process ends or
    // is reaped during the read (`ns/pid` of a reaped process gives
    // `EACCES`). The pidfd is the truth: an error after its exit is "ended".
    let gone = |e: io::Error| match readable(fd.as_raw_fd(), Duration::ZERO) {
        Ok(true) => Ok(None),
        _ => Err(e),
    };
    let stat = match fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(s) => s,
        Err(e) => return gone(e),
    };
    let pid_ns = match fs::metadata(format!("/proc/{pid}/ns/pid")) {
        Ok(m) => m.ino(),
        Err(e) => return gone(e),
    };
    let start = start_time(&stat)
        .ok_or_else(|| io::Error::other("cannot read the start time of a process"))?;
    // A pidfd is readable when its process exited. The init of a PID
    // namespace exits only after each process in it ended.
    if readable(fd.as_raw_fd(), Duration::ZERO)? {
        return Ok(None);
    }
    let boot = boot_id()?;
    Ok(Some((
        fd,
        Info {
            boot,
            pid,
            start,
            pid_ns,
        },
    )))
}

/// Field 22 of `/proc/<pid>/stat`. The name (field 2) can hold spaces and
/// `)`, so the fields are counted after the last `)`.
pub(crate) fn start_time(stat: &str) -> Option<u64> {
    stat.rsplit_once(')')?
        .1
        .split_whitespace()
        .nth(19)?
        .parse()
        .ok()
}

/// Reads the info line of `bwrap` (one JSON object, at most [`INFO_MAX`]
/// bytes, before `end`) and gives `child-pid`.
fn read_info(fd: OwnedFd, end: Instant) -> io::Result<i32> {
    let mut buf = Vec::new();
    let mut f = File::from(fd);
    while !buf.contains(&b'}') {
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() || !readable(f.as_raw_fd(), left)? {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "no info line from bwrap",
            ));
        }
        let mut chunk = [0u8; 512];
        let n = f.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.len() > INFO_MAX {
            return Err(io::Error::other("the info line of bwrap is too long"));
        }
    }
    parse_info(&buf).ok_or_else(|| io::Error::other("the info line of bwrap is not valid"))
}

/// Parses `{"child-pid": N, ...}`: the first JSON object of the bytes.
/// Other fields are not read.
pub(crate) fn parse_info(bytes: &[u8]) -> Option<i32> {
    let end = bytes.iter().position(|b| *b == b'}')? + 1;
    let v: serde_json::Value = serde_json::from_slice(&bytes[..end]).ok()?;
    let pid = i32::try_from(v.get("child-pid")?.as_u64()?).ok()?;
    (pid > 1).then_some(pid)
}

fn pipe() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0; 2];
    // SAFETY: fds is a valid array of two ints for pipe2 to write.
    if unsafe { libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) } != 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: pipe2 gave two new descriptors that nothing else owns.
    Ok(unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) })
}

/// True if `fd` is readable (data, end or error) in `wait`.
fn readable(fd: RawFd, wait: Duration) -> io::Result<bool> {
    let mut p = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let ms = wait.as_millis().min(i32::MAX as u128) as i32;
    loop {
        // SAFETY: p is one valid pollfd.
        let r = unsafe { libc::poll(&mut p, 1, ms) };
        if r >= 0 {
            return Ok(r > 0);
        }
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::Interrupted {
            return Err(e);
        }
    }
}

/// True if `e` says that the process ended: no `/proc/<pid>` entry
/// (`NotFound`), or a process that ended during the call (`ESRCH`).
fn exited(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::NotFound || e.raw_os_error() == Some(libc::ESRCH)
}

fn pidfd_open(pid: i32) -> io::Result<OwnedFd> {
    // SAFETY: pidfd_open takes a process ID and flags, and gives a new fd.
    let r = unsafe { libc::syscall(libc::SYS_pidfd_open, pid, 0) };
    if r < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the kernel gave a new descriptor that nothing else owns.
    Ok(unsafe { OwnedFd::from_raw_fd(r as RawFd) })
}

/// Sends `SIGKILL` to the process of `fd` (so the kernel kills each process
/// of its PID namespace) and waits for its end.
fn kill_and_wait(fd: &OwnedFd) -> io::Result<()> {
    // SAFETY: pidfd_send_signal with a valid pidfd and no siginfo.
    let r = unsafe {
        libc::syscall(
            libc::SYS_pidfd_send_signal,
            fd.as_raw_fd(),
            libc::SIGKILL,
            std::ptr::null::<libc::siginfo_t>(),
            0,
        )
    };
    if r != 0 {
        let e = io::Error::last_os_error();
        if e.raw_os_error() != Some(libc::ESRCH) {
            return Err(e);
        }
    }
    if readable(fd.as_raw_fd(), STOP_WAIT)? {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "the sandbox did not end in 5 seconds",
        ))
    }
}

/// Makes a TCP listener on `127.0.0.1:<port>` in the network namespace of
/// the process `init`: a forked child joins the user namespace that owns
/// that network namespace, then the network namespace, binds the listener
/// and sends it back.
fn listen_in(init: i32, port: u16, end: Instant) -> io::Result<TcpListener> {
    let net = CString::new(format!("/proc/{init}/ns/net")).expect("no NUL");
    let own_user = fs::metadata("/proc/self/ns/user")?.ino();
    let (parent, child_sock) = std::os::unix::net::UnixStream::pair()?;
    let sock = child_sock.as_raw_fd();
    // SAFETY: the child runs only system calls on values made before the
    // fork, then _exit. It allocates nothing and takes no lock.
    let pid = unsafe { libc::fork() };
    if pid < 0 {
        return Err(io::Error::last_os_error());
    }
    if pid == 0 {
        // SAFETY: the child of the fork; see above.
        unsafe { libc::_exit(join_and_bind(&net, own_user, port, sock)) };
    }
    drop(child_sock);
    let got = recv_fd(&parent, end.saturating_duration_since(Instant::now()));
    let mut status = 0;
    // SAFETY: waitpid on our own child, with a valid status pointer.
    unsafe { libc::waitpid(pid, &mut status, 0) };
    let fd = got.map_err(|e| {
        let code = if libc::WIFEXITED(status) {
            libc::WEXITSTATUS(status)
        } else {
            -1
        };
        io::Error::other(format!("no listener in the sandbox (step {code}): {e}"))
    })?;
    Ok(TcpListener::from(fd))
}

/// The child of [`listen_in`]. Gives the exit code: 0, or the failed step.
///
/// # Safety
///
/// Call only in the child of a fork: it changes the namespaces of the
/// process.
unsafe fn join_and_bind(net: &CString, own_user: u64, port: u16, sock: RawFd) -> i32 {
    let nfd = libc::open(net.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC);
    if nfd < 0 {
        return 10;
    }
    // NS_GET_USERNS: the user namespace that owns the network namespace.
    let ufd = libc::ioctl(nfd, 0xb701 as _);
    if ufd < 0 {
        return 11;
    }
    let mut st: libc::stat = std::mem::zeroed();
    if libc::fstat(ufd, &mut st) != 0 {
        return 12;
    }
    // A join of the own user namespace is refused (EINVAL); root with no
    // new user namespace needs no join.
    if st.st_ino != own_user && libc::setns(ufd, libc::CLONE_NEWUSER) != 0 {
        return 13;
    }
    if libc::setns(nfd, libc::CLONE_NEWNET) != 0 {
        return 14;
    }
    let s = libc::socket(libc::AF_INET, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0);
    if s < 0 {
        return 15;
    }
    let addr = libc::sockaddr_in {
        sin_family: libc::AF_INET as libc::sa_family_t,
        sin_port: port.to_be(),
        sin_addr: libc::in_addr {
            s_addr: u32::from_be_bytes([127, 0, 0, 1]).to_be(),
        },
        sin_zero: [0; 8],
    };
    if libc::bind(
        s,
        &addr as *const libc::sockaddr_in as *const libc::sockaddr,
        std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
    ) != 0
    {
        return 16;
    }
    if libc::listen(s, 16) != 0 {
        return 17;
    }
    if send_fd(sock, s) != 0 {
        return 18;
    }
    0
}

/// Receives one file descriptor from [`send_fd`], in `wait`.
fn recv_fd(sock: &std::os::unix::net::UnixStream, wait: Duration) -> io::Result<OwnedFd> {
    if !readable(sock.as_raw_fd(), wait)? {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "no listener in the time limit",
        ));
    }
    crate::egress::recv_fd(sock.as_raw_fd())?
        .ok_or_else(|| io::Error::other("the child sent no listener"))
}

/// Takes the helper end of the socket pair of the egress proxy:
/// [`EGRESS_FD`](crate::egress::EGRESS_FD), which the launch gives to the helper. It gets
/// close-on-exec, so no program of the session gets it.
fn take_channel() -> io::Result<OwnedFd> {
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    // SAFETY: fstat writes into `st`; fcntl has no memory effects.
    let ok = unsafe {
        libc::fstat(crate::egress::EGRESS_FD, &mut st) == 0
            && st.st_mode & libc::S_IFMT == libc::S_IFSOCK
            && libc::fcntl(crate::egress::EGRESS_FD, libc::F_SETFD, libc::FD_CLOEXEC) == 0
    };
    if !ok {
        return Err(io::Error::other(
            "the helper has no egress channel from the engine",
        ));
    }
    // SAFETY: the launch gave this descriptor to the helper, and nothing
    // else in the helper owns it.
    Ok(unsafe { OwnedFd::from_raw_fd(crate::egress::EGRESS_FD) })
}

/// Starts the relay thread: each connection to `listener` (in the sandbox)
/// goes to `127.0.0.1:<port>` of the host from the address `from`,
/// at most [`RELAY_MAX`] at a time, while the process of `engine` runs. The
/// threads end with the helper.
pub(crate) fn start_relay(
    listener: TcpListener,
    port: u16,
    from: Ipv4Addr,
    engine: OwnedFd,
) -> io::Result<()> {
    let open = Arc::new(AtomicUsize::new(0));
    thread::Builder::new()
        .name("iris-relay".into())
        .spawn(move || {
            for inner in listener.incoming() {
                let Ok(inner) = inner else {
                    // For example no file descriptor: wait, do not spin.
                    thread::sleep(Duration::from_millis(50));
                    continue;
                };
                // After the end of the engine, make no new connection.
                if readable(engine.as_raw_fd(), Duration::ZERO).unwrap_or(true) {
                    return; // the listener closes
                }
                if open.load(Ordering::SeqCst) >= RELAY_MAX {
                    continue; // dropped: closed at once
                }
                let Ok(outer) = crate::egress::connect_from(from, port) else {
                    continue;
                };
                open.fetch_add(1, Ordering::SeqCst);
                let count = open.clone();
                if thread::Builder::new()
                    .name("iris-relay-conn".into())
                    .spawn(move || {
                        relay(inner, outer);
                        count.fetch_sub(1, Ordering::SeqCst);
                    })
                    .is_err()
                {
                    open.fetch_sub(1, Ordering::SeqCst);
                }
            }
        })?;
    Ok(())
}

/// Copies the bytes of one connection in the two directions until both
/// ends close.
fn relay(a: TcpStream, b: TcpStream) {
    let (Ok(a2), Ok(b2)) = (a.try_clone(), b.try_clone()) else {
        return;
    };
    let Ok(back) = thread::Builder::new()
        .name("iris-relay-back".into())
        .spawn(move || copy(b2, a2))
    else {
        return; // all four handles close: the agent sees the end
    };
    copy(a, b);
    let _ = back.join();
}

fn copy(mut from: TcpStream, mut to: TcpStream) {
    let _ = io::copy(&mut from, &mut to);
    let _ = to.shutdown(Shutdown::Write);
    let _ = from.shutdown(Shutdown::Read);
}

#[cfg(test)]
#[path = "isolation_linux_tests.rs"]
mod tests;
