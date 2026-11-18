//! Tests of the shim and the handoff. They run on each system: the shim
//! takes a fake of launchd, and a thread of the test acts as the job
//! process.

use super::*;
use crate::isolation::macos::{Proc, Record, Sys};
use std::collections::BTreeMap;
use std::sync::Mutex;

/// The process facts that the shim reads: the coalition of each process.
struct FakeSys(Mutex<BTreeMap<i32, u64>>);

impl Sys for FakeSys {
    fn boot_time(&self) -> io::Result<String> {
        Ok("100.000001".into())
    }
    fn pids(&self) -> io::Result<Vec<i32>> {
        Ok(self.0.lock().unwrap().keys().copied().collect())
    }
    fn proc(&self, pid: i32) -> io::Result<Option<Proc>> {
        Ok(self.0.lock().unwrap().get(&pid).map(|c| Proc {
            coalition: *c,
            stopped: false,
            unique: 1,
        }))
    }
    fn counts(&self, _cid: u64) -> io::Result<Option<(u64, u64)>> {
        Ok(None)
    }
    fn signal(&self, _pid: i32, _sig: i32) -> io::Result<()> {
        Ok(())
    }
    fn run(&self, _argv: &[OsString], _wait: Duration) -> io::Result<i32> {
        Ok(0)
    }
    fn uid(&self) -> u32 {
        501
    }
}

/// What the fake job process does after the bootstrap.
#[derive(Clone, Copy)]
enum Job {
    /// Writes the record of coalition 50, connects and sends the code 7.
    Good,
    /// Connects with no record.
    NoRecord,
    /// Writes a record of coalition 51 but runs in coalition 50.
    OtherCoalition,
    /// Never starts.
    Silent,
}

struct FakeLaunchd {
    data: PathBuf,
    session: i64,
    job: Job,
    peer_uid: u32,
    bootouts: Mutex<Vec<String>>,
    got: Arc<Mutex<Option<(usize, Handoff)>>>,
}

impl Launchd for FakeLaunchd {
    fn bootstrap(&self, plist: &Path) -> io::Result<()> {
        let text = fs::read_to_string(plist)?;
        assert!(text.contains("<string>session-job</string>"), "{text}");
        let (data, session, job, got) =
            (self.data.clone(), self.session, self.job, self.got.clone());
        if matches!(job, Job::Silent) {
            return Ok(());
        }
        thread::spawn(move || {
            let sb = Seatbelt::with(&data, Box::new(FakeSys(Mutex::default()))).unwrap();
            let cid = match job {
                Job::OtherCoalition => 51,
                _ => 50,
            };
            if !matches!(job, Job::NoRecord) {
                let rec = Record {
                    boot: "100.000001".into(),
                    coalition: cid,
                    port: Some(4000),
                };
                sb.write_record(session, &rec).unwrap();
            }
            let f = files(&data, session);
            let Ok(s) = UnixStream::connect(&f.socket) else {
                return;
            };
            if let Ok((fds, msg)) = recv_with_fds(&s, HANDOFF_FDS) {
                *got.lock().unwrap() = Some((fds.len(), Handoff::decode(&msg).unwrap()));
                let _ = (&s).write_all(&[7]);
            }
        });
        Ok(())
    }
    fn bootout(&self, label: &str) -> io::Result<()> {
        self.bootouts.lock().unwrap().push(label.to_string());
        Ok(())
    }
    fn peer(&self, _stream: &UnixStream) -> io::Result<(u32, i32)> {
        Ok((self.peer_uid, 4242))
    }
    fn uid(&self) -> u32 {
        501
    }
}

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run_shim(
    name: &str,
    job: Job,
    peer_uid: u32,
) -> (Dir, i32, Vec<String>, Option<(usize, Handoff)>, Seatbelt) {
    let d = std::env::temp_dir().join(format!("iris-shim-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    let d = fs::canonicalize(&d).unwrap();
    // The job process 4242 runs in coalition 50.
    let sys = FakeSys(Mutex::new(BTreeMap::from([(4242, 50)])));
    let sb = Seatbelt::with(&d, Box::new(sys)).unwrap();
    let launchd = FakeLaunchd {
        data: d.clone(),
        session: 3,
        job,
        peer_uid,
        bootouts: Mutex::default(),
        got: Arc::default(),
    };
    let args: Vec<OsString> = vec!["3".into(), "/usr/bin/sandbox-exec".into(), "-p".into()];
    let env = vec![("HTTPS_PROXY".into(), "http://iris:t@127.0.0.1:4000".into())];
    let code = shim(&sb, &launchd, Path::new("/opt/iris/iris"), &d, &args, env);
    let bootouts = launchd.bootouts.lock().unwrap().clone();
    let got = launchd.got.lock().unwrap().take();
    (Dir(d), code, bootouts, got, sb)
}

#[test]
fn the_shim_hands_off_to_the_job_of_the_record_and_exits_with_its_code() {
    let (d, code, bootouts, got, _sb) = run_shim("good", Job::Good, 501);
    assert_eq!(code, 7);
    assert!(bootouts.is_empty());
    let (fds, h) = got.unwrap();
    assert_eq!(fds, HANDOFF_FDS);
    assert_eq!(h.args, ["3", "/usr/bin/sandbox-exec", "-p"]);
    assert_eq!(h.env[0].1, "http://iris:t@127.0.0.1:4000");
    let plist = fs::read_to_string(files(&d.0, 3).plist).unwrap();
    assert!(plist.contains("<string>4000</string>"), "{plist}");
    assert!(
        !plist.contains("iris:t"),
        "the token is not in the job definition"
    );
}

#[test]
fn the_shim_refuses_a_job_with_no_record_another_coalition_or_user() {
    for (name, job, uid) in [
        ("norecord", Job::NoRecord, 501),
        ("other", Job::OtherCoalition, 501),
        ("user", Job::Good, 502),
        ("silent", Job::Silent, 501),
    ] {
        let t = Instant::now();
        let (_d, code, bootouts, got, sb) = run_shim(name, job, uid);
        assert_eq!(code, 1, "{name}");
        assert!(got.is_none(), "{name}: the handoff went out");
        assert_eq!(bootouts, [sb.name(3)], "{name}");
        assert!(t.elapsed() < CONNECT_WAIT * 3, "{name}");
    }
}

#[test]
fn descriptors_and_a_message_cross_the_socket() {
    let (a, b) = UnixStream::pair().unwrap();
    let mut pipes = Vec::new();
    for _ in 0..HANDOFF_FDS {
        pipes.push(io::pipe().unwrap());
    }
    let raw: Vec<RawFd> = pipes.iter().map(|(_, w)| w.as_raw_fd()).collect();
    send_with_fds(&a, &raw, b"hello").unwrap();
    let (fds, msg) = recv_with_fds(&b, HANDOFF_FDS).unwrap();
    assert_eq!(msg, b"hello");
    for (i, fd) in fds.into_iter().enumerate() {
        // SAFETY: fcntl on an open descriptor.
        let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFD) };
        assert_eq!(flags & libc::FD_CLOEXEC, libc::FD_CLOEXEC);
        let mut w = fs::File::from(fd);
        w.write_all(&[i as u8]).unwrap();
        drop(w);
        let mut got = [0u8; 1];
        pipes[i].0.read_exact(&mut got).unwrap();
        assert_eq!(got[0], i as u8);
    }
    // The wrong count of descriptors is refused.
    send_with_fds(&a, &raw[..2], b"x").unwrap();
    assert!(recv_with_fds(&b, HANDOFF_FDS).is_err());
}

/// A failed receive gives its error and reads no control data.
#[test]
fn a_failed_receive_gives_its_error() {
    let (_a, b) = UnixStream::pair().unwrap();
    b.set_nonblocking(true).unwrap();
    let e = recv_with_fds(&b, HANDOFF_FDS).unwrap_err();
    assert_eq!(e.kind(), io::ErrorKind::WouldBlock);
}

/// Random handoffs read back the same; random bytes never pass as a
/// handoff other than its own encoding.
#[test]
fn a_handoff_reads_back_and_random_bytes_do_not_pass() {
    let seed = std::cell::Cell::new(3u64);
    let next = |n: u64| {
        seed.set(
            seed.get()
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407),
        );
        (seed.get() >> 33) % n
    };
    for _ in 0..5_000 {
        let word = |nul: bool| -> OsString {
            let n = next(8);
            let v: Vec<u8> = (0..n)
                .map(|_| {
                    let c = next(256) as u8;
                    if !nul && (c == 0 || c == b'=') {
                        b'a'
                    } else {
                        c
                    }
                })
                .collect();
            OsString::from_vec(v)
        };
        let args: Vec<OsString> = (0..next(4)).map(|_| word(true)).collect();
        let env: Vec<(OsString, OsString)> = (0..next(4))
            .map(|_| {
                let mut k = word(false);
                if k.is_empty() {
                    k = "K".into();
                }
                let v = OsString::from_vec(
                    word(false)
                        .into_vec()
                        .into_iter()
                        .map(|c| c.max(1))
                        .collect(),
                );
                (k, v)
            })
            .collect();
        let h = Handoff { args, env };
        let e = h.encode();
        assert_eq!(Handoff::decode(&e).as_ref(), Some(&h));
        // A cut or an extra byte does not pass.
        let cut = next(e.len() as u64) as usize;
        assert!(Handoff::decode(&e[..cut]).is_none());
        let mut more = e.clone();
        more.push(next(256) as u8);
        assert!(Handoff::decode(&more).is_none());
        // Random bytes: either no handoff, or one that encodes to them.
        let r: Vec<u8> = (0..next(40)).map(|_| next(4) as u8).collect();
        if let Some(x) = Handoff::decode(&r) {
            assert_eq!(x.encode(), r);
        }
    }
    let bad = Handoff {
        args: vec![],
        env: vec![("A=B".into(), "c".into())],
    };
    assert!(Handoff::decode(&bad.encode()).is_none());
}

#[test]
fn the_job_definition_escapes_each_string() {
    let p = plist(
        "com.iris.session.a.1",
        &["/a<b>&\"c".into(), SESSION_JOB.into()],
    );
    assert!(
        p.contains("<string>/a&lt;b&gt;&amp;&quot;c</string>"),
        "{p}"
    );
    assert!(p.contains("<key>RunAtLoad</key>\n<true/>"));
    assert!(p.contains("<key>KeepAlive</key>\n<false/>"));
}

#[test]
fn the_egress_port_comes_from_the_proxy_url() {
    let env = |v: &str| vec![(OsString::from("HTTPS_PROXY"), OsString::from(v))];
    assert_eq!(
        egress_port(&env("http://iris:ab@127.0.0.1:4000")),
        Some(4000)
    );
    assert_eq!(egress_port(&env("http://127.0.0.1:4001")), Some(4001));
    assert_eq!(egress_port(&env("http://example.test:4000")), None);
    assert_eq!(egress_port(&[]), None);
}

#[test]
fn the_job_refuses_bad_arguments_before_any_change() {
    let d = std::env::temp_dir();
    let sb = Seatbelt::with(&d, Box::new(FakeSys(Mutex::default()))).unwrap();
    for args in [
        vec![],
        vec!["0", "-"],
        vec!["3"],
        vec!["3", "x"],
        vec!["3", "-", "x"],
    ] {
        let a: Vec<OsString> = args.into_iter().map(OsString::from).collect();
        assert_eq!(job(&sb, &d, &a), hold::BAD_ARGS);
    }
}

/// The job process of the test below: it runs only in a child process of
/// the test binary, with `IRIS_TEST_JOB_DIR` set, because `job` changes the
/// streams, the folder and the environment of its process.
#[test]
fn job_child() {
    let Some(dir) = std::env::var_os("IRIS_TEST_JOB_DIR") else {
        return;
    };
    let me = std::process::id() as i32;
    let sys = FakeSys(Mutex::new(BTreeMap::from([(me, 50)])));
    let dir = PathBuf::from(dir);
    let sb = Seatbelt::with(&dir, Box::new(sys)).unwrap();
    let code = job(&sb, &dir, &["3".into(), "4000".into()]);
    std::process::exit(code);
}

/// The side of the shim of a job child process.
struct Started {
    child: std::process::Child,
    sock: UnixStream,
    engine: io::PipeWriter,
    status: io::PipeReader,
    dir: PathBuf,
    /// The descriptors of the test process that a child inherits.
    inherited: Vec<i32>,
    _dir: Dir,
}

/// Starts `job_child` in a new folder and hands off `agent` (a `/bin/sh -c`
/// script) with the engine pipe, the log, the status pipe and `work`.
fn start_job(name: &str, agent: &str) -> Started {
    let d = std::env::temp_dir().join(format!("iris-job-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("work")).unwrap();
    let d = fs::canonicalize(&d).unwrap();
    let guard = Dir(d.clone());
    let f = files(&d, 3);
    fs::create_dir_all(f.socket.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(&f.socket).unwrap();
    // SAFETY: fcntl on numbers that may or may not be open.
    let inherited: Vec<i32> = (3..64)
        .filter(|&n| {
            let f = unsafe { libc::fcntl(n, libc::F_GETFD) };
            f >= 0 && f & libc::FD_CLOEXEC == 0
        })
        .collect();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "macos_job::tests::job_child", "--test-threads=1"])
        .env("IRIS_TEST_JOB_DIR", &d)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let (s, _) = listener.accept().unwrap();
    let (engine_r, engine_w) = io::pipe().unwrap();
    let (log_r, log_w) = io::pipe().unwrap();
    let (status_r, status_w) = io::pipe().unwrap();
    let work = fs::File::open(d.join("work")).unwrap();
    let fds = [
        engine_r.as_raw_fd(),
        log_w.as_raw_fd(),
        status_w.as_raw_fd(),
        work.as_raw_fd(),
    ];
    let h = Handoff {
        args: vec!["3".into(), "/bin/sh".into(), "-c".into(), agent.into()],
        env: vec![
            ("IRIS_T".into(), "handed".into()),
            ("PATH".into(), "/usr/bin:/bin".into()),
        ],
    };
    send_with_fds(&s, &fds, &h.encode()).unwrap();
    drop((engine_r, log_w, status_w, work, log_r));
    Started {
        child,
        sock: s,
        engine: engine_w,
        status: status_r,
        dir: d,
        inherited,
        _dir: guard,
    }
}

/// A valid handoff: the job writes the record, the hold waits for `start`
/// on the engine pipe, the agent runs in the handed-off folder with the
/// handed-off environment and only the three streams, and the shim gets
/// the exit code.
#[test]
fn the_job_runs_the_hold_with_the_handoff() {
    let mut j = start_job(
        "run",
        "pwd > seen; echo \"$IRIS_T\" >> seen; [ -z \"$IRIS_TEST_JOB_DIR\" ] && echo clean >> seen; (launchctl print gui/$(id -u) >/dev/null 2>&1 && echo launchd >> seen) 2>/dev/null; exec > fds; for n in $(seq 3 63); do [ -e /dev/fd/$n ] && echo $n; done; true",
    );
    let d = j.dir.clone();
    // The record came before the connection.
    let sb = Seatbelt::with(&d, Box::new(FakeSys(Mutex::default()))).unwrap();
    let rec = sb.record(3).unwrap().unwrap();
    assert_eq!((rec.coalition, rec.port), (50, Some(4000)));
    // No agent before `start`.
    thread::sleep(Duration::from_millis(200));
    assert!(!d.join("work/seen").exists());
    j.engine.write_all(b"start\n").unwrap();
    let mut status = String::new();
    j.status.read_to_string(&mut status).unwrap();
    assert!(status.starts_with("started "), "{status}");
    assert!(status.contains("exited 0"), "{status}");
    let mut code = [9u8; 1];
    (&j.sock).read_exact(&mut code).unwrap();
    assert_eq!(code[0], 0);
    assert!(j.child.wait().unwrap().success());
    let seen = fs::read_to_string(d.join("work/seen")).unwrap();
    assert_eq!(
        seen,
        format!("{}\nhanded\nclean\n", d.join("work").display())
    );
    // The line "launchd" is absent: on macOS the job gave the agent no
    // bootstrap port, so `launchctl print` failed (Linux has no launchctl).
    // Each open descriptor of the agent above 2 came from the test
    // process itself (open with no close-on-exec when the test started),
    // never from the handoff: the engine pipe, the log, the status pipe or
    // the folder. A stat of /dev/fd/N opens no descriptor, and `exec >`
    // keeps no copy of the old fd 1 (a redirect of the loop would).
    let fds = fs::read_to_string(d.join("work/fds")).unwrap();
    for n in fds.split_whitespace() {
        assert!(j.inherited.contains(&n.parse::<i32>().unwrap()), "{fds}");
    }
    assert!(sb.home(3).is_dir() && sb.tmp(3).is_dir());
}

/// The end of the shim stops a running agent, while the engine pipe stays
/// open (spec, step 5).
#[test]
fn the_end_of_the_shim_stops_the_agent() {
    let mut j = start_job("shim", "echo up > up; exec sleep 60");
    j.engine.write_all(b"start\n").unwrap();
    let up = j.dir.join("work/up");
    let t = Instant::now();
    while !up.exists() {
        assert!(
            t.elapsed() < Duration::from_secs(10),
            "the agent did not start"
        );
        thread::sleep(Duration::from_millis(20));
    }
    drop(j.sock);
    let t = Instant::now();
    loop {
        if j.child.try_wait().unwrap().is_some() {
            break;
        }
        assert!(t.elapsed() < Duration::from_secs(20), "the job did not end");
        thread::sleep(Duration::from_millis(20));
    }
    // The status pipe ends: no process of the session holds it.
    let mut status = String::new();
    j.status.read_to_string(&mut status).unwrap();
    assert!(status.starts_with("started "), "{status}");
    drop(j.engine);
}
