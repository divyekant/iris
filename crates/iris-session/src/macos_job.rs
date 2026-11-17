//! The launchd job of a macOS session (`docs/specs/2026-11-17-session-sandbox-macos.md`,
//! "The launchd job of a session"): the shim, which the engine starts as
//! the helper, and the job process, which launchd starts in a new resource
//! coalition and which runs the hold helper.
//!
//! The steps build on each system, with a [`Launchd`] for the calls of
//! macOS, so that CI tests them on Linux with a fake.

use std::ffi::{OsStr, OsString};
use std::fs::{self, OpenOptions};
use std::io::{self, IoSlice, IoSliceMut, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use iris_job::format_session_id;

use crate::hold;
use crate::isolation::macos::Seatbelt;
use crate::isolation::{Command, Isolation};

/// The hidden first argument of the job process of a session:
/// `iris session-job <data dir> <session> <port or ->`.
pub const SESSION_JOB: &str = "session-job";
/// The longest wait of the shim for the job process.
pub const CONNECT_WAIT: Duration = Duration::from_secs(1);
/// The largest handoff message.
const HANDOFF_MAX: usize = 1024 * 1024;
/// The count of file descriptors in the handoff: 0, 1, 2 and the folder.
const HANDOFF_FDS: usize = 4;

/// The calls of macOS that the shim makes.
pub trait Launchd {
    /// `launchctl bootstrap gui/<uid> <plist>`.
    fn bootstrap(&self, plist: &Path) -> io::Result<()>;
    /// `launchctl bootout gui/<uid>/<label>`. Not loaded is not an error.
    fn bootout(&self, label: &str) -> io::Result<()>;
    /// The user ID and the process ID of the peer of `stream`
    /// (`getpeereid` and `LOCAL_PEERPID`).
    fn peer(&self, stream: &UnixStream) -> io::Result<(u32, i32)>;
    /// The user ID of this process.
    fn uid(&self) -> u32;
}

/// What the shim gives the job process, other than the descriptors: the
/// hold arguments and the whole environment of the shim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Handoff {
    pub args: Vec<OsString>,
    pub env: Vec<(OsString, OsString)>,
}

impl Handoff {
    /// Each item as a 4-byte length (big-endian) and its bytes: the count of
    /// arguments, each argument, the count of variables, each name and
    /// value.
    pub fn encode(&self) -> Vec<u8> {
        fn item(out: &mut Vec<u8>, b: &[u8]) {
            out.extend_from_slice(&(b.len() as u32).to_be_bytes());
            out.extend_from_slice(b);
        }
        let mut out = Vec::new();
        out.extend_from_slice(&(self.args.len() as u32).to_be_bytes());
        for a in &self.args {
            item(&mut out, a.as_bytes());
        }
        out.extend_from_slice(&(self.env.len() as u32).to_be_bytes());
        for (k, v) in &self.env {
            item(&mut out, k.as_bytes());
            item(&mut out, v.as_bytes());
        }
        out
    }

    /// The reverse of [`Handoff::encode`]. `None` for each other input,
    /// also for bytes after the end, a variable name that is empty or holds
    /// `=`, and a NUL byte in a name or a value.
    pub fn decode(b: &[u8]) -> Option<Handoff> {
        let mut r = Reader { b, at: 0 };
        let mut args = Vec::new();
        for _ in 0..r.count()? {
            args.push(r.bytes()?);
        }
        let mut env = Vec::new();
        for _ in 0..r.count()? {
            let k = r.bytes()?;
            let v = r.bytes()?;
            let bad_name = k.is_empty() || k.as_bytes().iter().any(|c| *c == b'=' || *c == 0);
            if bad_name || v.as_bytes().contains(&0) {
                return None;
            }
            env.push((k, v));
        }
        (r.at == b.len()).then_some(Handoff { args, env })
    }
}

/// A reader of the items of a handoff message.
struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn u32(&mut self) -> Option<usize> {
        let v = self.b.get(self.at..self.at.checked_add(4)?)?;
        self.at += 4;
        Some(u32::from_be_bytes(v.try_into().ok()?) as usize)
    }

    /// A count of items: each item takes 4 bytes at least.
    fn count(&mut self) -> Option<usize> {
        let n = self.u32()?;
        (n <= (self.b.len() - self.at) / 4).then_some(n)
    }

    fn bytes(&mut self) -> Option<OsString> {
        let n = self.u32()?;
        let v = self.b.get(self.at..self.at.checked_add(n)?)?;
        self.at += n;
        Some(OsString::from_vec(v.to_vec()))
    }
}

/// The launchd job definition of a session, as a property list. Each
/// string is escaped for XML.
pub fn plist(label: &str, program: &[OsString]) -> String {
    fn esc(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }
    let args: String = program
        .iter()
        .map(|a| format!("    <string>{}</string>\n", esc(&a.to_string_lossy())))
        .collect();
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \
         \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\">\n<dict>\n\
         <key>Label</key>\n<string>{}</string>\n\
         <key>ProgramArguments</key>\n<array>\n{args}</array>\n\
         <key>RunAtLoad</key>\n<true/>\n\
         <key>KeepAlive</key>\n<false/>\n\
         </dict>\n</plist>\n",
        esc(label)
    )
}

/// Sends `fds` and `msg` on `sock` (`SCM_RIGHTS`), then the rest of `msg`.
pub fn send_with_fds(sock: &UnixStream, fds: &[RawFd], msg: &[u8]) -> io::Result<()> {
    let len = (msg.len() as u32).to_be_bytes();
    let space = unsafe { libc::CMSG_SPACE(std::mem::size_of_val(fds) as u32) } as usize;
    let mut cbuf = vec![0u8; space];
    let iov = [IoSlice::new(&len)];
    // SAFETY: the header points at buffers that live for the call, and
    // the control buffer has the space of one SCM_RIGHTS message.
    let n = unsafe {
        let mut hdr: libc::msghdr = std::mem::zeroed();
        hdr.msg_iov = iov.as_ptr() as *mut libc::iovec;
        hdr.msg_iovlen = 1;
        hdr.msg_control = cbuf.as_mut_ptr().cast();
        hdr.msg_controllen = space as _;
        let c = libc::CMSG_FIRSTHDR(&hdr);
        (*c).cmsg_level = libc::SOL_SOCKET;
        (*c).cmsg_type = libc::SCM_RIGHTS;
        (*c).cmsg_len = libc::CMSG_LEN(std::mem::size_of_val(fds) as u32) as _;
        std::ptr::copy_nonoverlapping(fds.as_ptr(), libc::CMSG_DATA(c).cast(), fds.len());
        libc::sendmsg(sock.as_raw_fd(), &hdr, 0)
    };
    if n != 4 {
        return Err(io::Error::last_os_error());
    }
    (&*sock).write_all(msg)
}

/// Receives `count` descriptors and a message of at most [`HANDOFF_MAX`]
/// bytes. Each received descriptor is owned, so it closes on each error.
pub fn recv_with_fds(sock: &UnixStream, count: usize) -> io::Result<(Vec<OwnedFd>, Vec<u8>)> {
    let mut len = [0u8; 4];
    let space = unsafe { libc::CMSG_SPACE((count * 4) as u32) } as usize;
    let mut cbuf = vec![0u8; space];
    let mut fds: Vec<OwnedFd> = Vec::new();
    let mut iov = [IoSliceMut::new(&mut len)];
    // SAFETY: as in send_with_fds; each descriptor of the reply is taken
    // into an OwnedFd at once.
    let n = unsafe {
        let mut hdr: libc::msghdr = std::mem::zeroed();
        hdr.msg_iov = iov.as_mut_ptr() as *mut libc::iovec;
        hdr.msg_iovlen = 1;
        hdr.msg_control = cbuf.as_mut_ptr().cast();
        hdr.msg_controllen = space as _;
        let n = libc::recvmsg(sock.as_raw_fd(), &mut hdr, 0);
        let mut c = libc::CMSG_FIRSTHDR(&hdr);
        while !c.is_null() {
            if (*c).cmsg_level == libc::SOL_SOCKET && (*c).cmsg_type == libc::SCM_RIGHTS {
                let data = libc::CMSG_DATA(c) as *const RawFd;
                let k = ((*c).cmsg_len as usize - libc::CMSG_LEN(0) as usize) / 4;
                for i in 0..k {
                    fds.push(OwnedFd::from_raw_fd(std::ptr::read_unaligned(data.add(i))));
                }
            }
            c = libc::CMSG_NXTHDR(&hdr, c);
        }
        if hdr.msg_flags & libc::MSG_CTRUNC != 0 {
            return Err(io::Error::other("the handoff has too many descriptors"));
        }
        n
    };
    if n != 4 {
        return Err(io::Error::other("the handoff has no header"));
    }
    if fds.len() != count {
        return Err(io::Error::other(
            "the handoff has the wrong count of descriptors",
        ));
    }
    let len = u32::from_be_bytes(len) as usize;
    if len > HANDOFF_MAX {
        return Err(io::Error::other("the handoff is too large"));
    }
    let mut msg = vec![0u8; len];
    (&*sock).read_exact(&mut msg)?;
    Ok((fds, msg))
}

/// The paths of the shim and the job of one session.
struct Files {
    socket: PathBuf,
    plist: PathBuf,
}

fn files(data_dir: &Path, session: i64) -> Files {
    let sessions = data_dir.join("sessions");
    let id = format_session_id(session);
    Files {
        socket: sessions.join(format!("{id}.hold")),
        plist: sessions.join(format!("{id}.plist")),
    }
}

/// The shim: `iris session-hold <data dir> <session> <program> [<arg>...]`
/// on macOS. It loads the launchd job of the session, gives it the three
/// streams, the working folder, the environment and the hold arguments,
/// and exits with the exit code of the job process (1 if none came).
pub fn shim(
    seatbelt: &Seatbelt,
    launchd: &dyn Launchd,
    iris: &Path,
    data_dir: &Path,
    args: &[OsString],
    env: Vec<(OsString, OsString)>,
) -> i32 {
    let Some(session) = args
        .first()
        .and_then(|s| s.to_str())
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|s| *s > 0)
    else {
        return hold::BAD_ARGS;
    };
    match shim_run(seatbelt, launchd, iris, data_dir, session, args, env) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("iris: the launchd job of the session did not start: {e}");
            1
        }
    }
}

fn shim_run(
    seatbelt: &Seatbelt,
    launchd: &dyn Launchd,
    iris: &Path,
    data_dir: &Path,
    session: i64,
    args: &[OsString],
    env: Vec<(OsString, OsString)>,
) -> io::Result<i32> {
    let f = files(data_dir, session);
    let sessions = f.socket.parent().expect("a sessions folder");
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(sessions)?;
    let _ = fs::remove_file(&f.socket);
    let listener = UnixListener::bind(&f.socket)?;
    fs::set_permissions(
        &f.socket,
        std::os::unix::fs::PermissionsExt::from_mode(0o600),
    )?;
    let label = seatbelt.name(session);
    let port = egress_port(&env).map_or("-".to_string(), |p| p.to_string());
    let program: Vec<OsString> = vec![
        iris.into(),
        SESSION_JOB.into(),
        data_dir.into(),
        session.to_string().into(),
        port.into(),
    ];
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&f.plist)?;
    file.write_all(plist(&label, &program).as_bytes())?;
    drop(file);
    launchd.bootstrap(&f.plist)?;
    let stream = match accept_job(seatbelt, launchd, &listener, session) {
        Ok(s) => s,
        Err(e) => {
            let _ = launchd.bootout(&label);
            let _ = fs::remove_file(&f.socket);
            return Err(e);
        }
    };
    let _ = fs::remove_file(&f.socket);
    let cwd = fs::File::open(".")?;
    let fds = [0, 1, 2, cwd.as_raw_fd()];
    let handoff = Handoff {
        args: args.to_vec(),
        env,
    };
    send_with_fds(&stream, &fds, &handoff.encode())?;
    drop(cwd);
    // The exit code of the job process, then the end.
    let mut code = [0u8; 1];
    Ok(match (&stream).read(&mut code) {
        Ok(1) => i32::from(code[0]),
        _ => 1,
    })
}

/// Waits up to [`CONNECT_WAIT`] for the job process, and takes it only if
/// it runs as this user, in the coalition of the record of the session.
fn accept_job(
    seatbelt: &Seatbelt,
    launchd: &dyn Launchd,
    listener: &UnixListener,
    session: i64,
) -> io::Result<UnixStream> {
    let end = Instant::now() + CONNECT_WAIT;
    listener.set_nonblocking(true)?;
    loop {
        match listener.accept() {
            Ok((s, _)) => {
                s.set_nonblocking(false)?;
                let (uid, pid) = launchd.peer(&s)?;
                if uid != launchd.uid() {
                    continue;
                }
                let Some(rec) = seatbelt.record(session)? else {
                    continue;
                };
                if seatbelt.coalition_of(pid)? != Some(rec.coalition) {
                    continue;
                }
                return Ok(s);
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                if Instant::now() >= end {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "the job process did not connect in 1 second",
                    ));
                }
                thread::sleep(Duration::from_millis(5));
            }
            Err(e) => return Err(e),
        }
    }
}

/// The port of `HTTPS_PROXY` in `env` (with or without the token).
fn egress_port(env: &[(OsString, OsString)]) -> Option<u16> {
    let v = env.iter().find(|(k, _)| k == "HTTPS_PROXY")?.1.to_str()?;
    let rest = v.strip_prefix("http://")?;
    let host = rest.rsplit_once('@').map_or(rest, |(_, h)| h);
    host.strip_prefix("127.0.0.1:")?.parse().ok()
}

/// The job process: `iris session-job <data dir> <session> <port or ->`.
/// It writes the record, takes the handoff from the shim, sets the
/// streams, the folder and the environment, makes the private home and
/// `/tmp`, and runs the hold helper. It sends the exit code of the hold to
/// the shim.
pub fn job(seatbelt: &Seatbelt, data_dir: &Path, args: &[OsString]) -> i32 {
    let parsed = (|| {
        let session = args
            .first()?
            .to_str()?
            .parse::<i64>()
            .ok()
            .filter(|s| *s > 0)?;
        let port = match args.get(1)?.to_str()? {
            "-" => None,
            p => Some(p.parse::<u16>().ok().filter(|p| *p > 0)?),
        };
        (args.len() == 2).then_some((session, port))
    })();
    let Some((session, port)) = parsed else {
        return hold::BAD_ARGS;
    };
    // The record comes first: the shim checks it before the handoff.
    if seatbelt.write_own_record(session, port).is_err() {
        return 1;
    }
    let f = files(data_dir, session);
    let Ok(stream) = UnixStream::connect(&f.socket) else {
        return 1;
    };
    let Ok((fds, msg)) = recv_with_fds(&stream, HANDOFF_FDS) else {
        return 1;
    };
    let Some(handoff) = Handoff::decode(&msg) else {
        return 1;
    };
    if apply(seatbelt, session, &fds, &handoff).is_err() {
        return 1;
    }
    let code = run_hold(seatbelt, session, &handoff.args, &stream);
    let _ = (&stream).write_all(&[code.clamp(0, 255) as u8]);
    code
}

/// The streams, the working folder and the environment of the handoff,
/// and the private folders of the session.
fn apply(seatbelt: &Seatbelt, session: i64, fds: &[OwnedFd], h: &Handoff) -> io::Result<()> {
    for (i, fd) in fds[..3].iter().enumerate() {
        // SAFETY: dup2 on two open descriptors.
        if unsafe { libc::dup2(fd.as_raw_fd(), i as RawFd) } < 0 {
            return Err(io::Error::last_os_error());
        }
    }
    // SAFETY: fchdir on an open folder descriptor.
    if unsafe { libc::fchdir(fds[3].as_raw_fd()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    for (k, _) in std::env::vars_os() {
        std::env::remove_var(k);
    }
    for (k, v) in &h.env {
        std::env::set_var(k, v);
    }
    for dir in [seatbelt.home(session), seatbelt.tmp(session)] {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&dir)
            .or_else(|e| {
                if e.kind() == io::ErrorKind::AlreadyExists {
                    Ok(())
                } else {
                    Err(e)
                }
            })?;
    }
    Ok(())
}

/// Runs the hold with an input that ends when the pipe from the engine
/// ends or when the shim ends (spec, step 5).
fn run_hold(seatbelt: &Seatbelt, session: i64, args: &[OsString], shim: &UnixStream) -> i32 {
    let Some(program) = args.get(1) else {
        return hold::BAD_ARGS;
    };
    if args.first().and_then(|s| s.to_str()) != Some(&session.to_string()) {
        return hold::BAD_ARGS;
    }
    let command = Command {
        program: PathBuf::from(program),
        args: args[2..].to_vec(),
    };
    let Ok((input, writer)) = io::pipe() else {
        return 1;
    };
    let writer = Arc::new(Mutex::new(Some(writer)));
    let w = writer.clone();
    let engine = thread::Builder::new()
        .name("iris-job-pipe".into())
        .spawn(move || {
            let mut stdin = io::stdin();
            let mut buf = [0u8; 64];
            loop {
                match stdin.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let mut g = w.lock().unwrap_or_else(|e| e.into_inner());
                        let sent = g.as_mut().is_some_and(|p| p.write_all(&buf[..n]).is_ok());
                        if !sent {
                            break;
                        }
                    }
                }
            }
            w.lock().unwrap_or_else(|e| e.into_inner()).take();
        });
    let Ok(sock) = shim.try_clone() else {
        return 1;
    };
    let w = writer.clone();
    let watch = thread::Builder::new()
        .name("iris-job-shim".into())
        .spawn(move || {
            let mut b = [0u8; 1];
            let _ = (&sock).read(&mut b);
            w.lock().unwrap_or_else(|e| e.into_inner()).take();
        });
    if engine.is_err() || watch.is_err() {
        writer.lock().unwrap_or_else(|e| e.into_inner()).take();
    }
    hold::hold(
        seatbelt as &dyn Isolation,
        session,
        &command,
        input,
        io::stderr(),
        io::stdout(),
    )
}

/// `iris session-hold <data dir> <hold args>`: on macOS the shim, on
/// Linux the hold helper in the isolation of the system.
pub fn helper_main(data_dir: &Path, args: &[OsString]) -> i32 {
    #[cfg(target_os = "macos")]
    {
        use crate::isolation::macos_sys::{MacLaunchd, MacSys};
        let seatbelt = match Seatbelt::with(data_dir, Box::new(MacSys)) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("iris: {e}");
                return 1;
            }
        };
        let Ok(iris) = std::env::current_exe().and_then(fs::canonicalize) else {
            return 1;
        };
        let real = fs::canonicalize(data_dir).unwrap_or_else(|_| data_dir.to_path_buf());
        shim(
            &seatbelt,
            &MacLaunchd,
            &iris,
            &real,
            args,
            std::env::vars_os().collect(),
        )
    }
    #[cfg(not(target_os = "macos"))]
    match crate::isolation::system(data_dir) {
        Ok(isolation) => hold::hold_main(&*isolation, args),
        Err(e) => {
            eprintln!("iris: {e}");
            1
        }
    }
}

/// `iris session-job <data dir> <session> <port or ->`: the job process of
/// a macOS session. Each other system refuses it.
pub fn job_main(data_dir: &Path, args: &[OsString]) -> i32 {
    #[cfg(target_os = "macos")]
    {
        use crate::isolation::macos_sys::MacSys;
        match Seatbelt::with(data_dir, Box::new(MacSys)) {
            Ok(s) => job(&s, data_dir, args),
            Err(_) => 1,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (data_dir, args);
        eprintln!("iris: session-job runs only on macOS");
        1
    }
}

/// True if `a` is the hidden argument of the job process.
pub fn is_job(a: &OsStr) -> bool {
    a == SESSION_JOB
}

#[cfg(test)]
#[path = "macos_job_tests.rs"]
mod tests;
