//! The Linux sandbox of a session (`docs/specs/2026-11-17-session-sandbox.md`,
//! "Linux"): bubblewrap (`bwrap`) with new namespaces, a file view of the
//! system folders and the workspace, and one relay from the network
//! namespace of the session to its egress port.

use std::ffi::{CString, OsStr, OsString};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
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
    /// empty `tmpfs`; `reads` are extra read-only paths.
    fn view(&self, home: Option<&Path>, reads: &[PathBuf], workspace: &Path) -> Vec<OsString> {
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
        if let Some(home) = home.filter(|h| h.is_absolute() && *h != Path::new("/")) {
            push(&["--tmpfs".as_ref(), home.as_os_str()]);
        }
        for r in reads {
            push(&["--ro-bind".as_ref(), r.as_os_str(), r.as_os_str()]);
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
        let mut args = self.view(None, &[], &self.data_dir);
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

    /// [`Isolation::spawn`] with the egress port of the session.
    pub(crate) fn spawn_with(
        &self,
        session: i64,
        command: &mut std::process::Command,
        egress_port: Option<u16>,
    ) -> io::Result<Child> {
        let (info_r, info_w) = pipe()?;
        let (block_r, block_w) = pipe()?;
        let (iw, br) = (info_w.as_raw_fd(), block_r.as_raw_fd());
        // SAFETY: the closure runs in the child between fork and exec. It
        // calls only dup2, which is async-signal-safe. dup2 clears
        // FD_CLOEXEC on the new descriptor, so bwrap gets fds 3 and 4.
        unsafe {
            command.pre_exec(move || {
                // Move both out of the target range first, so one dup2
                // cannot close the other.
                let iw2 = libc::fcntl(iw, libc::F_DUPFD_CLOEXEC, 10);
                let br2 = libc::fcntl(br, libc::F_DUPFD_CLOEXEC, 10);
                if iw2 < 0
                    || br2 < 0
                    || libc::dup2(iw2, INFO_FD) < 0
                    || libc::dup2(br2, BLOCK_FD) < 0
                {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        // The engine: the relay connects only while it runs.
        // SAFETY: getppid has no memory effects.
        let engine = pidfd_open(unsafe { libc::getppid() })?;
        let mut child = command.spawn()?;
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
            if let Some(port) = egress_port {
                // The proxy of the session takes only this address.
                let from = crate::egress::session_peer(session)
                    .ok_or_else(|| io::Error::other("the session has no session address"))?;
                let listener = listen_in(info.pid, port, deadline)?;
                start_relay(listener, port, from, engine)?;
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
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|h| fs::canonicalize(&h).unwrap_or(h));
        let workspace = fs::canonicalize(workspace).unwrap_or_else(|_| workspace.to_path_buf());
        let mut a = self.view(home.as_deref(), &reads, &workspace);
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
        self.spawn_with(session, command, port)
    }
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
        Err(e) if e.raw_os_error() == Some(libc::ESRCH) => return Ok(None),
        Err(e) => return Err(e),
    };
    let gone = |e: io::Error| match e.kind() {
        io::ErrorKind::NotFound => Ok(None),
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

/// Sends `fd` on the Unix socket `sock` (`SCM_RIGHTS`). Gives 0 or -1.
///
/// # Safety
///
/// Only system calls on the stack: safe in the child of a fork.
unsafe fn send_fd(sock: RawFd, fd: RawFd) -> i32 {
    let mut byte = [0u8; 1];
    let mut iov = libc::iovec {
        iov_base: byte.as_mut_ptr() as *mut libc::c_void,
        iov_len: 1,
    };
    let mut space = [0u64; 4];
    let mut msg: libc::msghdr = std::mem::zeroed();
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = space.as_mut_ptr() as *mut libc::c_void;
    msg.msg_controllen = libc::CMSG_SPACE(std::mem::size_of::<RawFd>() as u32) as _;
    let cmsg = libc::CMSG_FIRSTHDR(&msg);
    (*cmsg).cmsg_level = libc::SOL_SOCKET;
    (*cmsg).cmsg_type = libc::SCM_RIGHTS;
    (*cmsg).cmsg_len = libc::CMSG_LEN(std::mem::size_of::<RawFd>() as u32) as _;
    std::ptr::write_unaligned(libc::CMSG_DATA(cmsg) as *mut RawFd, fd);
    if libc::sendmsg(sock, &msg, libc::MSG_NOSIGNAL) == 1 {
        0
    } else {
        -1
    }
}

/// Receives one file descriptor from [`send_fd`], in `wait`.
fn recv_fd(sock: &std::os::unix::net::UnixStream, wait: Duration) -> io::Result<OwnedFd> {
    if !readable(sock.as_raw_fd(), wait)? {
        return Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "no listener in the time limit",
        ));
    }
    let mut byte = [0u8; 1];
    let mut iov = libc::iovec {
        iov_base: byte.as_mut_ptr() as *mut libc::c_void,
        iov_len: 1,
    };
    let mut space = [0u64; 4];
    // SAFETY: msg points to valid buffers that live for the call.
    unsafe {
        let mut msg: libc::msghdr = std::mem::zeroed();
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        msg.msg_control = space.as_mut_ptr() as *mut libc::c_void;
        msg.msg_controllen = std::mem::size_of_val(&space) as _;
        let n = libc::recvmsg(sock.as_raw_fd(), &mut msg, libc::MSG_CMSG_CLOEXEC);
        if n < 0 {
            return Err(io::Error::last_os_error());
        }
        let cmsg = libc::CMSG_FIRSTHDR(&msg);
        if n != 1
            || cmsg.is_null()
            || (*cmsg).cmsg_level != libc::SOL_SOCKET
            || (*cmsg).cmsg_type != libc::SCM_RIGHTS
        {
            return Err(io::Error::other("the child sent no listener"));
        }
        let fd = std::ptr::read_unaligned(libc::CMSG_DATA(cmsg) as *const RawFd);
        Ok(OwnedFd::from_raw_fd(fd))
    }
}

/// Starts the relay thread: each connection to `listener` (in the sandbox)
/// goes to `127.0.0.1:<port>` of the host from the session address `from`,
/// at most [`RELAY_MAX`] at a time, while the process of `engine` runs. The
/// threads end with the helper.
pub(crate) fn start_relay(
    listener: TcpListener,
    port: u16,
    from: std::net::Ipv4Addr,
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
                // A second guard: the proxy of another session refuses the
                // session address in all cases (an engine crash frees the
                // port). After the end of the engine, make no new connection.
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
