//! The macOS sandbox of a session (`docs/specs/2026-11-17-session-sandbox-macos.md`):
//! a Seatbelt profile (`sandbox-exec`) for the agent, the launchd job of the
//! session for its own resource coalition, and an exact `ended` from the
//! counters of that coalition.
//!
//! The logic uses a [`Sys`] for each fact of the processes and each signal,
//! so the tests of CI (Linux) run it with a fake. The real [`Sys`]
//! (`isolation_macos_sys.rs`) builds on macOS only, and the Mac acceptance
//! tests it.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use iris_job::format_session_id;
use iris_store::FileLock;
use sha2::{Digest, Sha256};

use super::{Command, Isolation};

/// The program that applies a Seatbelt profile.
pub const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";
/// The longest run of the freeze and kill loop of one `stop`.
const STOP_WAIT: Duration = Duration::from_secs(5);
/// The exit codes of `launchctl bootout` for a job that is not loaded:
/// 3 ("No such process") and 113 ("Could not find service").
const NOT_LOADED: [i32; 2] = [3, 113];
/// The longest run of `check`.
const CHECK_WAIT: Duration = Duration::from_secs(5);
/// The pause between two scans of the freeze, so that a `SIGSTOP` takes
/// effect.
const SCAN_PAUSE: Duration = Duration::from_millis(2);

/// The facts of one process.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Proc {
    /// The resource coalition ID.
    pub coalition: u64,
    /// True if the status is `SSTOP`.
    pub stopped: bool,
    /// The unique ID of the process in this boot (`p_uniqueid`).
    pub unique: u64,
}

/// The process facts and the signals that the macOS sandbox uses.
pub trait Sys: Send + Sync {
    /// The boot time (`kern.boottime`), as text with no space.
    fn boot_time(&self) -> io::Result<String>;
    /// The process IDs of the system (`proc_listallpids`).
    fn pids(&self) -> io::Result<Vec<i32>>;
    /// The facts of `pid`. `Ok(None)` only if no process has the ID
    /// (`ESRCH`); each other failure of a read is an error.
    fn proc(&self, pid: i32) -> io::Result<Option<Proc>>;
    /// `(tasks_started, tasks_exited)` of the coalition `cid`, or `None` if
    /// the coalition is gone (`ESRCH`).
    fn counts(&self, cid: u64) -> io::Result<Option<(u64, u64)>>;
    /// Sends `sig` to `pid`.
    fn signal(&self, pid: i32, sig: i32) -> io::Result<()>;
    /// Runs `argv` with no input and no output, and gives its exit code.
    /// It kills the program and gives an error after `wait`.
    fn run(&self, argv: &[OsString], wait: Duration) -> io::Result<i32>;
    /// The user ID of this process.
    fn uid(&self) -> u32;
}

/// The record of a session: the boot, the coalition of its launchd job and
/// its egress port.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub boot: String,
    pub coalition: u64,
    pub port: Option<u16>,
}

impl Record {
    fn to_line(&self) -> String {
        let port = self.port.map_or("-".to_string(), |p| p.to_string());
        format!("{} {} {}\n", self.boot, self.coalition, port)
    }

    /// The record is one line, `<sec>.<usec> <coalition> <port or ->`,
    /// with 6 digits of microseconds. Each other text is not valid.
    fn parse(text: &str) -> Option<Record> {
        let line = text.strip_suffix('\n')?;
        let p: Vec<&str> = line.split(' ').collect();
        let [boot, cid, port] = p[..] else {
            return None;
        };
        let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
        let (sec, usec) = boot.split_once('.')?;
        if !digits(sec) || !digits(usec) || usec.len() != 6 || !digits(cid) {
            return None;
        }
        if port != "-" && !digits(port) {
            return None;
        }
        Some(Record {
            boot: boot.to_string(),
            coalition: cid.parse().ok().filter(|c| *c > 0)?,
            port: match port {
                "-" => None,
                p => Some(p.parse().ok().filter(|p| *p > 0)?),
            },
        })
    }
}

/// The paths of the profile of one session. Each one is a real path.
#[derive(Clone, Debug)]
pub struct Paths {
    pub workspace: PathBuf,
    pub home: PathBuf,
    pub tmp: PathBuf,
    /// The adapter program and the `iris` program.
    pub programs: Vec<PathBuf>,
}

/// The macOS isolation.
pub struct Seatbelt {
    sessions: PathBuf,
    /// The full SHA-256 of the real path of the data folder, in hex.
    tag: String,
    sys: Box<dyn Sys>,
}

impl Seatbelt {
    /// The data folder is resolved first, so that two paths of one folder
    /// give one label.
    pub fn with(data_dir: &Path, sys: Box<dyn Sys>) -> Result<Self, String> {
        let data_dir = fs::canonicalize(data_dir).map_err(|e| {
            format!(
                "the data folder {} cannot be resolved: {e}",
                data_dir.display()
            )
        })?;
        let tag = Sha256::digest(data_dir.as_os_str().as_encoded_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        Ok(Self {
            tag,
            sessions: data_dir.join("sessions"),
            sys,
        })
    }

    fn file(&self, session: i64, ext: &str) -> PathBuf {
        self.sessions
            .join(format!("{}.{ext}", format_session_id(session)))
    }

    /// The private home of the session.
    pub fn home(&self, session: i64) -> PathBuf {
        self.file(session, "home")
    }

    /// The private `/tmp` of the session.
    pub fn tmp(&self, session: i64) -> PathBuf {
        self.file(session, "tmp")
    }

    fn sessions_dir(&self) -> io::Result<()> {
        fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&self.sessions)
    }

    /// Writes the record, complete or absent after a crash: a synced
    /// temporary file, then a link to the name, which fails if the name
    /// exists.
    pub fn write_record(&self, session: i64, rec: &Record) -> io::Result<()> {
        self.sessions_dir()?;
        let record = self.file(session, "sandbox");
        let tmp = self.file(session, "sandbox.tmp");
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        f.write_all(rec.to_line().as_bytes())?;
        f.sync_all()?;
        fs::hard_link(&tmp, &record)?;
        File::open(&self.sessions)?.sync_all()?;
        let _ = fs::remove_file(&tmp);
        Ok(())
    }

    /// The record of the session, or `None` if there is none.
    pub fn record(&self, session: i64) -> io::Result<Option<Record>> {
        match fs::read_to_string(self.file(session, "sandbox")) {
            Ok(t) => Record::parse(&t)
                .map(Some)
                .ok_or_else(|| io::Error::other("the sandbox record is not valid")),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// True if no process of the coalition of `rec` runs. Each other
    /// result, also an error of the counters, is false.
    fn coalition_ended(&self, rec: &Record) -> io::Result<bool> {
        if self.sys.boot_time()? != rec.boot {
            return Ok(true); // the system started again
        }
        Ok(match self.sys.counts(rec.coalition) {
            Ok(None) => true,
            Ok(Some((started, exited))) => started == exited,
            Err(_) => false,
        })
    }

    fn read_journal(&self, session: i64) -> io::Result<Vec<i32>> {
        match fs::read_to_string(self.file(session, "stops")) {
            Ok(t) => {
                parse_journal(&t).ok_or_else(|| io::Error::other("the stop journal is not valid"))
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Vec::new()),
            Err(e) => Err(e),
        }
    }

    /// Appends `pid` to the journal, with `fsync`, before its `SIGSTOP`.
    fn journal(&self, session: i64, pid: i32) -> io::Result<()> {
        let mut f = OpenOptions::new()
            .append(true)
            .create(true)
            .mode(0o600)
            .open(self.file(session, "stops"))?;
        writeln!(f, "{pid}")?;
        f.sync_all()
    }

    /// Resolves each entry of the journal (spec, "Recovery of the journal")
    /// and keeps only the entries that are not resolved. Gives their count.
    fn recover(&self, session: i64, cid: Option<u64>) -> io::Result<usize> {
        let entries = self.read_journal(session)?;
        if entries.is_empty() {
            return Ok(0);
        }
        let mut left: Vec<i32> = Vec::new();
        for pid in entries {
            if !left.contains(&pid) && !self.resolved(pid, cid) {
                left.push(pid);
            }
        }
        let tmp = self.file(session, "stops.tmp");
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)?;
        for pid in &left {
            writeln!(f, "{pid}")?;
        }
        f.sync_all()?;
        fs::rename(&tmp, self.file(session, "stops"))?;
        File::open(&self.sessions)?.sync_all()?;
        Ok(left.len())
    }

    /// The table of the spec. The process that has the ID now is the one
    /// that Iris can have stopped.
    fn resolved(&self, pid: i32, cid: Option<u64>) -> bool {
        // A read that fails leaves the entry.
        let ok = |p: io::Result<Option<Proc>>| match p {
            Ok(p) => p.is_none_or(|p| Some(p.coalition) == cid || !p.stopped),
            Err(_) => false,
        };
        let first = self.sys.proc(pid);
        if first.is_err() {
            return false;
        }
        if ok(first) {
            return true;
        }
        self.sys.signal(pid, libc::SIGCONT).is_ok() && ok(self.sys.proc(pid))
    }

    /// One scan of the freeze: it stops each member of `cid` that runs.
    /// Gives the frozen set (process ID to unique ID) when the scan finds
    /// each live member stopped, their count equals the live count of the
    /// coalition, and no task started during the scan. Else `None`.
    fn freeze(&self, session: i64, cid: u64) -> io::Result<Option<BTreeMap<i32, u64>>> {
        let Some((started, _)) = self.sys.counts(cid)? else {
            return Ok(Some(BTreeMap::new()));
        };
        let mut frozen = BTreeMap::new();
        let mut settled = true;
        for pid in self.sys.pids()? {
            // A process that cannot be read (for example of another user)
            // is skipped: if it is a member, the count below does not match.
            let Ok(Some(p)) = self.sys.proc(pid) else {
                continue;
            };
            if p.coalition != cid {
                continue;
            }
            if p.stopped {
                frozen.insert(pid, p.unique);
                continue;
            }
            settled = false;
            self.journal(session, pid)?;
            let _ = self.sys.signal(pid, libc::SIGSTOP);
            // The ID can have gone to a process of another coalition
            // between the reads: let that process run again.
            let now = self.sys.proc(pid).ok().flatten().map(|q| q.coalition);
            if now != Some(cid) {
                let _ = self.sys.signal(pid, libc::SIGCONT);
            }
        }
        let Some((now, exited)) = self.sys.counts(cid)? else {
            return Ok(Some(BTreeMap::new()));
        };
        let live = now.saturating_sub(exited);
        Ok((settled && now == started && frozen.len() as u64 == live).then_some(frozen))
    }

    /// `SIGKILL` to each member of the frozen set that is still stopped, in
    /// the coalition and the same process.
    fn kill(&self, cid: u64, frozen: &BTreeMap<i32, u64>) {
        for (&pid, &unique) in frozen {
            let same = self
                .sys
                .proc(pid)
                .ok()
                .flatten()
                .is_some_and(|p| p.stopped && p.coalition == cid && p.unique == unique);
            if same {
                let _ = self.sys.signal(pid, libc::SIGKILL);
            }
        }
    }

    /// `launchctl bootout` of the job of the session, until `end`. A job
    /// that is not loaded is not an error.
    fn bootout(&self, session: i64, end: Instant) -> io::Result<()> {
        let argv: Vec<OsString> = vec![
            "/bin/launchctl".into(),
            "bootout".into(),
            format!("gui/{}/{}", self.sys.uid(), self.name(session)).into(),
        ];
        match self
            .sys
            .run(&argv, end.saturating_duration_since(Instant::now()))?
        {
            0 => Ok(()),
            c if NOT_LOADED.contains(&c) => Ok(()),
            c => Err(io::Error::other(format!(
                "launchctl bootout: exit code {c}"
            ))),
        }
    }

    fn stop_with(&self, session: i64, wait: Duration) -> io::Result<()> {
        self.sessions_dir()?;
        let _lock = FileLock::take(&self.file(session, "stop.lock"))?;
        let rec = self.record(session)?;
        let cid = rec.as_ref().map(|r| r.coalition);
        // 0. The hold helper inside the coalition: the stop of the engine,
        // outside it, does the work.
        let me = self
            .sys
            .proc(std::process::id() as i32)?
            .map(|p| p.coalition);
        if cid.is_some() && me == cid {
            return Ok(());
        }
        let end = Instant::now() + wait;
        // A journal of an earlier boot names processes that are gone: its
        // IDs now belong to other processes, so it is removed, not
        // resolved. Without a record no freeze ran, so no journal is valid.
        let boot = self.sys.boot_time()?;
        let Some(rec) = rec.filter(|r| r.boot == boot) else {
            match fs::remove_file(self.file(session, "stops")) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                _ => return Ok(()),
            }
        };
        self.recover(session, cid)?;
        // 1. The job, inside the time limit of the stop.
        self.bootout(session, end)?;
        // 2. Freeze, then kill, until the coalition holds no process.
        let mut ended = self.coalition_ended(&rec)?;
        while !ended && Instant::now() < end {
            if let Some(frozen) = self.freeze(session, rec.coalition)? {
                self.kill(rec.coalition, &frozen);
            }
            std::thread::sleep(SCAN_PAUSE);
            ended = self.coalition_ended(&rec)?;
        }
        // 3. The journal again, then the result.
        let left = self.recover(session, Some(rec.coalition))?;
        if !ended {
            let live = match self.sys.counts(rec.coalition) {
                Ok(Some((s, e))) => s.saturating_sub(e).to_string(),
                _ => "an unknown count of".to_string(),
            };
            return Err(io::Error::other(format!(
                "the session still has {live} live processes"
            )));
        }
        if left > 0 {
            return Err(io::Error::other(format!(
                "{left} processes that the stop froze are not resolved"
            )));
        }
        Ok(())
    }
}

impl Isolation for Seatbelt {
    /// In 5 seconds: `sandbox-exec` runs `/usr/bin/true` with the profile
    /// of a test session, the GUI domain of launchd exists, and the
    /// counters of the coalition of this process give a result.
    fn check(&self) -> Result<(), String> {
        let end = Instant::now() + CHECK_WAIT;
        let left = || end.saturating_duration_since(Instant::now());
        let test = self.sessions.join("check");
        let paths = Paths {
            workspace: test.clone(),
            home: test.clone(),
            tmp: test,
            programs: vec!["/usr/bin/true".into()],
        };
        let mut argv: Vec<OsString> = vec![SANDBOX_EXEC.into()];
        argv.extend(sandbox_args(&paths, None));
        argv.push("/usr/bin/true".into());
        let sandbox = "the macOS sandbox (sandbox-exec) does not work";
        match self.sys.run(&argv, left()) {
            Ok(0) => {}
            Ok(c) => return Err(format!("{sandbox}: exit code {c}")),
            Err(e) => return Err(format!("{sandbox}: {e}")),
        }
        let domain = format!("gui/{}", self.sys.uid());
        let argv: Vec<OsString> = vec!["/bin/launchctl".into(), "print".into(), domain.into()];
        if self.sys.run(&argv, left()).ok() != Some(0) {
            return Err(
                "agent sessions on macOS need a person signed in to the Mac \
                        (no launchd GUI domain)"
                    .into(),
            );
        }
        let me = self
            .sys
            .proc(std::process::id() as i32)
            .ok()
            .flatten()
            .ok_or("the coalition of this process cannot be read")?;
        match self.sys.counts(me.coalition) {
            Ok(Some(_)) => {}
            Ok(None) => return Err("the coalition of this process is gone".into()),
            Err(e) => return Err(format!("the coalition counters cannot be read: {e}")),
        }
        if left().is_zero() {
            return Err("the check of the macOS sandbox took more than 5 seconds".into());
        }
        Ok(())
    }

    /// `sandbox-exec` with the profile of the session, then `env` with the
    /// private home and `/tmp`, and the folder of `iris` first in `PATH`.
    fn command(
        &self,
        session: i64,
        workspace: &Path,
        egress_port: Option<u16>,
        program: &Path,
        args: &[String],
    ) -> Command {
        let real = |p: &Path| fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        let program = real(program);
        let iris = std::env::current_exe().map(|p| real(&p)).ok();
        let paths = Paths {
            workspace: real(workspace),
            home: real(&self.home(session)),
            tmp: real(&self.tmp(session)),
            programs: [Some(program.clone()), iris.clone()]
                .into_iter()
                .flatten()
                .collect(),
        };
        let mut a = sandbox_args(&paths, egress_port);
        a.push("/usr/bin/env".into());
        let var = |k: &str, v: &Path| {
            let mut s = OsString::from(format!("{k}="));
            s.push(v);
            s
        };
        a.push(var("HOME", &paths.home));
        a.push(var("TMPDIR", &paths.tmp));
        if let Some(dir) = iris.as_deref().and_then(Path::parent) {
            let mut path = var("PATH", dir);
            if let Some(p) = std::env::var_os("PATH") {
                path.push(":");
                path.push(p);
            }
            a.push(path);
        }
        a.push(program.into_os_string());
        a.extend(args.iter().map(OsString::from));
        Command {
            program: SANDBOX_EXEC.into(),
            args: a,
        }
    }

    /// The label of the launchd job: the full SHA-256 of the data folder
    /// and the session ID.
    fn name(&self, session: i64) -> String {
        format!("com.iris.session.{}.{session}", self.tag)
    }

    fn stop(&self, session: i64) -> io::Result<()> {
        self.stop_with(session, STOP_WAIT)
    }

    fn ended(&self, session: i64) -> io::Result<bool> {
        match self.record(session)? {
            None => Ok(true),
            Some(rec) => self.coalition_ended(&rec),
        }
    }
}

/// The options of `sandbox-exec` for the profile of `paths`: each
/// parameter, then the profile text.
fn sandbox_args(paths: &Paths, egress_port: Option<u16>) -> Vec<OsString> {
    let (text, params) = profile(paths, egress_port);
    let mut a: Vec<OsString> = Vec::new();
    for (k, v) in params {
        let mut d = OsString::from(format!("{k}="));
        d.push(v);
        a.extend(["-D".into(), d]);
    }
    a.extend(["-p".into(), text.into()]);
    a
}

/// The stop journal: one process ID (greater than 0) on each line, each
/// line with its end. `None` for each other text.
fn parse_journal(text: &str) -> Option<Vec<i32>> {
    if text.is_empty() {
        return Some(Vec::new());
    }
    text.strip_suffix('\n')?
        .split('\n')
        .map(|l| {
            let ok = !l.is_empty() && l.bytes().all(|b| b.is_ascii_digit());
            ok.then(|| l.parse::<i32>().ok().filter(|p| *p > 0))
                .flatten()
        })
        .collect()
}

/// The system folders that the agent can read (never all of `/System`,
/// and not `/usr/local`).
const SYSTEM_READS: &[&str] = &[
    "/usr/bin",
    "/usr/lib",
    "/usr/libexec",
    "/usr/sbin",
    "/usr/share",
    "/bin",
    "/sbin",
    "/System/Library",
    "/System/Cryptexes",
    "/System/Volumes/Preboot/Cryptexes",
    "/Library/Apple",
    "/private/var/db/dyld",
    "/private/etc/ssl/certs",
];

/// The single files that the agent can read.
const FILE_READS: &[&str] = &[
    "/dev/null",
    "/dev/zero",
    "/dev/random",
    "/dev/urandom",
    "/private/etc/hosts",
    "/private/etc/passwd",
    "/private/etc/group",
    "/private/etc/localtime",
    "/private/etc/ssl/cert.pem",
];

/// The `sysctl` names that tell the system and the hardware. Never
/// `kern.proc*`, which gives the arguments and the environment of other
/// processes.
const SYSCTL_NAMES: &[&str] = &[
    "kern.ostype",
    "kern.osrelease",
    "kern.osversion",
    "kern.osproductversion",
    "kern.version",
    "kern.hostname",
    "kern.boottime",
    "kern.argmax",
    "kern.maxfiles",
    "kern.maxfilesperproc",
    "kern.usrstack64",
    "kern.secure_kernel",
    "vm.loadavg",
    "vm.pagesize",
    "sysctl.proc_translated",
];

/// The Seatbelt profile of a session, and its parameters (`-D`). Each path
/// of the session goes in a parameter, never in the profile text, so no
/// path can change the profile.
pub fn profile(p: &Paths, egress_port: Option<u16>) -> (String, Vec<(String, PathBuf)>) {
    let mut params: Vec<(String, PathBuf)> = Vec::new();
    let mut param = |path: &Path| {
        let k = format!("P{}", params.len());
        params.push((k.clone(), path.to_path_buf()));
        format!("(param \"{k}\")")
    };
    let mut s = String::from("(version 1)\n(deny default)\n");
    s.push_str("(allow process-exec process-fork)\n");
    s.push_str("(allow signal (target same-sandbox))\n");
    s.push_str("(allow process-info* (target same-sandbox))\n");
    s.push_str(
        "(allow sysctl-read (sysctl-name-prefix \"hw.\") (sysctl-name-prefix \"machdep.cpu.\")",
    );
    for n in SYSCTL_NAMES {
        s.push_str(&format!(" (sysctl-name \"{n}\")"));
    }
    s.push_str(")\n");
    let mut reads: Vec<String> = SYSTEM_READS
        .iter()
        .map(|d| format!("(subpath \"{d}\")"))
        .collect();
    reads.extend(FILE_READS.iter().map(|f| format!("(literal \"{f}\")")));
    for f in &p.programs {
        reads.push(format!("(literal {})", param(f)));
    }
    // Each folder above an allowed path, for the resolution of paths only.
    let mut above: Vec<PathBuf> = Vec::new();
    let allowed = SYSTEM_READS
        .iter()
        .chain(FILE_READS)
        .map(PathBuf::from)
        .chain(p.programs.iter().cloned())
        .chain([p.workspace.clone(), p.home.clone(), p.tmp.clone()]);
    for path in allowed {
        for a in path.ancestors().skip(1) {
            if !above.iter().any(|x| x == a) {
                above.push(a.to_path_buf());
            }
        }
    }
    let above: Vec<String> = above
        .iter()
        .map(|a| format!("(literal {})", param(a)))
        .collect();
    s.push_str(&format!("(allow file-read* {})\n", reads.join(" ")));
    s.push_str(&format!("(allow file-read-metadata {})\n", above.join(" ")));
    s.push_str("(allow file-write* (literal \"/dev/null\"))\n");
    if let Some(port) = egress_port {
        s.push_str(&format!(
            "(allow network-outbound (remote tcp \"localhost:{port}\"))\n"
        ));
    }
    let socket = param(&p.workspace.join(".iris").join("socket"));
    s.push_str(&format!(
        "(allow network-outbound (remote unix-socket (path-literal {socket})))\n"
    ));
    // The second path of the user volume never gives access. The folders of
    // the session come after this rule, by their own real path.
    s.push_str("(deny file-read* file-write* (subpath \"/System/Volumes/Data\"))\n");
    let dirs: Vec<String> = [&p.workspace, &p.home, &p.tmp]
        .iter()
        .map(|d| format!("(subpath {})", param(d)))
        .collect();
    s.push_str(&format!(
        "(allow file-read* file-write* {})\n",
        dirs.join(" ")
    ));
    (s, params)
}

#[cfg(test)]
#[path = "isolation_macos_tests.rs"]
mod tests;
