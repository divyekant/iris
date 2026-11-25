//! Tests of the Linux sandbox. They run real `bwrap` sandboxes: the engine
//! area needs the package `bubblewrap`.

use super::*;
use std::io::BufRead;
use std::os::unix::process::CommandExt;
use std::sync::atomic::AtomicU32;

static NEXT: AtomicU32 = AtomicU32::new(0);

/// A data folder with a key, an engine socket file, and two workspaces.
struct Data {
    dir: PathBuf,
    ws: PathBuf,
}

impl Data {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "iris-sbx-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&dir);
        let ws = dir.join("workspaces/j-1");
        fs::create_dir_all(&ws).unwrap();
        fs::create_dir_all(dir.join("workspaces/j-2")).unwrap();
        fs::write(dir.join("key"), "KEY-MARKER").unwrap();
        fs::write(dir.join("iris.sock"), "SOCK-MARKER").unwrap();
        fs::write(dir.join("workspaces/j-2/mail.md"), "OTHER-MARKER").unwrap();
        Self { dir, ws }
    }
    fn iso(&self) -> Bwrap {
        Bwrap::new(&self.dir).expect("the engine area needs bubblewrap")
    }
}

impl Drop for Data {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// Runs `program args` in the sandbox of `session` and gives its output.
fn run(d: &Data, session: i64, port: Option<u16>, program: &str, args: &[&str]) -> String {
    let iso = d.iso();
    let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
    let c = iso.command(session, &d.ws, port, Path::new(program), &args, &[], None);
    let mut cmd = std::process::Command::new(&c.program);
    cmd.args(&c.args)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    let child = iso.spawn_with(session, &mut cmd, port).unwrap();
    let out = child.wait_with_output().unwrap();
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn sh(d: &Data, session: i64, script: &str) -> String {
    run(d, session, None, "/bin/sh", &["-c", script])
}

/// The processes of the PID namespace `ns`, as host process IDs.
fn in_namespace(ns: u64) -> Vec<i32> {
    fs::read_dir("/proc")
        .unwrap()
        .filter_map(|e| e.ok()?.file_name().to_str()?.parse::<i32>().ok())
        .filter(|pid| {
            fs::metadata(format!("/proc/{pid}/ns/pid"))
                .map(|m| m.ino())
                .ok()
                == Some(ns)
                && !fs::read_to_string(format!("/proc/{pid}/stat"))
                    .map(|s| {
                        s.rsplit(')')
                            .next()
                            .unwrap_or("")
                            .trim_start()
                            .starts_with('Z')
                    })
                    .unwrap_or(true)
        })
        .collect()
}

fn wait_for(mut f: impl FnMut() -> bool, secs: u64) -> bool {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        if f() {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    f()
}

#[test]
fn check_passes_here_and_fails_with_no_bwrap() {
    let d = Data::new();
    d.iso().check().unwrap();
    let none = Bwrap::with(PathBuf::from("/nonexistent/bwrap"), &d.dir).unwrap();
    let e = none.check().unwrap_err();
    assert!(e.contains("bwrap"), "{e}");
    assert_eq!(find_bwrap(Some(OsStr::new("relative:"))), find_bwrap(None));
}

#[test]
fn the_view_hides_the_data_folder_and_keeps_the_workspace() {
    let d = Data::new();
    let data = d.dir.display();
    let out = sh(
        &d,
        1,
        &format!(
            "cat {data}/key {data}/iris.sock {data}/workspaces/j-2/mail.md 2>/dev/null; \
             ls -A {data}; echo; echo home: $(ls -A \"$HOME\"); \
             echo note > notes.md && echo ws-ok; \
             (echo x > /etc/iris-test) 2>/dev/null || echo etc-ro; \
             (echo x > /usr/iris-test) 2>/dev/null || echo usr-ro; \
             (echo x > /iris-test) 2>/dev/null || echo root-ro; \
             (echo x > /usr/local/iris-test) 2>/dev/null || echo local-ro; \
             (echo x > {data}/iris-test) 2>/dev/null || echo data-ro; \
             (echo x > /tmp/iris-test) 2>/dev/null && echo tmp-rw; \
             echo local: $(ls -A /usr/local); \
             echo ssl: $(ls -A /etc/ssl /etc/pki/tls 2>/dev/null); \
             (head -c 1 /etc/ssl/cert.pem >/dev/null 2>&1 && echo ca-ok); \
             pwd"
        ),
    );
    for marker in ["KEY-MARKER", "SOCK-MARKER", "OTHER-MARKER"] {
        assert!(!out.contains(marker), "{marker} visible: {out}");
    }
    // Only the path to the workspace exists in the data folder.
    assert!(out.starts_with("workspaces\n"), "{out}");
    // The private home is empty, except the path to the iris program when
    // that program is in the home folder.
    let home = PathBuf::from(std::env::var_os("HOME").unwrap());
    let exe = fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
    let allowed = exe
        .strip_prefix(&home)
        .ok()
        .and_then(|p| p.components().next())
        .map(|c| c.as_os_str().to_string_lossy().into_owned());
    let line = out.lines().find(|l| l.starts_with("home:")).unwrap();
    for name in line["home:".len()..].split_whitespace() {
        assert_eq!(Some(name), allowed.as_deref(), "home not empty: {out}");
    }
    assert!(out.contains("\nlocal:\n"), "/usr/local not empty: {out}");
    // The certificate authority files are in the view; the folders of the
    // private keys are not.
    let ssl = out.lines().find(|l| l.starts_with("ssl:")).unwrap();
    assert!(!ssl.contains("private"), "{out}");
    if fs::metadata("/etc/ssl/cert.pem").is_ok() {
        assert!(out.contains("ca-ok"), "{out}");
    }
    assert!(
        out.contains("ws-ok")
            && out.contains("etc-ro")
            && out.contains("usr-ro")
            && out.contains("root-ro")
            && out.contains("local-ro")
            && out.contains("data-ro")
            && out.contains("tmp-rw"),
        "{out}"
    );
    assert!(
        out.trim_end().ends_with(&d.ws.display().to_string()),
        "{out}"
    );
    assert_eq!(fs::read_to_string(d.ws.join("notes.md")).unwrap(), "note\n");
}

#[test]
fn a_socket_or_a_file_outside_the_view_is_not_reached() {
    let d = Data::new();
    // A Unix socket in the data folder, and one in a folder of the host
    // outside the view, with a credential file beside it.
    let outside = d.dir.with_extension("run");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("token"), "RUN-MARKER").unwrap();
    let a = std::os::unix::net::UnixListener::bind(d.dir.join("engine.sock")).unwrap();
    let b = std::os::unix::net::UnixListener::bind(outside.join("agent.sock")).unwrap();
    let script = format!(
        "import socket\n\
         for p in ['{}', '{}']:\n\
         \x20   try:\n\
         \x20       socket.socket(socket.AF_UNIX).connect(p); print('REACHED', p)\n\
         \x20   except OSError: print('blocked')\n\
         for p in ['{}', '/etc/hostname', '/etc/shadow']:\n\
         \x20   try:\n\
         \x20       print('READ', open(p).read())\n\
         \x20   except OSError: print('hidden')\n\
         print('passwd', len(open('/etc/passwd').read()) > 0)\n",
        d.dir.join("engine.sock").display(),
        outside.join("agent.sock").display(),
        outside.join("token").display(),
    );
    let out = run(&d, 12, None, "/usr/bin/python3", &["-c", &script]);
    let _ = fs::remove_dir_all(&outside);
    drop((a, b));
    assert!(!out.contains("REACHED") && !out.contains("READ"), "{out}");
    assert_eq!(out.matches("blocked").count(), 2, "{out}");
    assert_eq!(out.matches("hidden").count(), 3, "{out}");
    assert!(out.contains("passwd True"), "{out}");
}

#[test]
fn a_program_inside_the_data_folder_does_not_show_the_data_folder() {
    let d = Data::new();
    let tool = d.dir.join("bin/tool");
    fs::create_dir_all(tool.parent().unwrap()).unwrap();
    fs::write(&tool, "#!/bin/sh\necho tool-ran\n").unwrap();
    fs::set_permissions(&tool, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let iso = d.iso();
    let c = iso.command(13, &d.ws, None, &tool, &[], &[], None);
    let mut cmd = std::process::Command::new(&c.program);
    cmd.args(&c.args)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let out = iso
        .spawn_with(13, &mut cmd, None)
        .unwrap()
        .wait_with_output()
        .unwrap();
    // The empty mount of the data folder hides the program too: it does
    // not run, and nothing of the data folder is read.
    assert!(!out.status.success());
    assert!(!String::from_utf8_lossy(&out.stdout).contains("tool-ran"));
}

#[test]
fn the_network_is_only_the_egress_port() {
    let d = Data::new();
    let egress = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = egress.local_addr().unwrap().port();
    let other = TcpListener::bind("127.0.0.1:0").unwrap();
    let other_port = other.local_addr().unwrap().port();
    let got = thread::spawn(move || {
        let (mut s, _) = egress.accept().unwrap();
        let mut b = [0u8; 2];
        s.read_exact(&mut b).unwrap();
        s.write_all(b"ok").unwrap();
        b
    });
    let script = format!(
        "import socket\n\
         s = socket.create_connection(('127.0.0.1', {port}), timeout=5)\n\
         s.sendall(b'hi'); print('egress', s.recv(2).decode())\n\
         for host, p in [('127.0.0.1', {other_port}), ('1.1.1.1', 443), ('10.0.0.1', 80)]:\n\
         \x20   try:\n\
         \x20       socket.create_connection((host, p), timeout=2); print('REACHED', host)\n\
         \x20   except OSError as e: print('blocked', host)\n\
         try:\n\
         \x20   u = socket.socket(socket.AF_INET, socket.SOCK_DGRAM); u.sendto(b'x', ('8.8.8.8', 53)); print('REACHED udp')\n\
         except OSError: print('blocked udp')\n"
    );
    let out = run(&d, 2, Some(port), "/usr/bin/python3", &["-c", &script]);
    assert_eq!(&got.join().unwrap(), b"hi");
    assert!(out.contains("egress ok"), "{out}");
    assert!(!out.contains("REACHED"), "{out}");
    assert_eq!(out.matches("blocked").count(), 4, "{out}");
    drop(other);
}

#[test]
fn a_session_with_no_port_has_no_listener() {
    let d = Data::new();
    // The TCP tables of the namespace: no socket in the state LISTEN (0A).
    let out = sh(
        &d,
        3,
        "cat /proc/net/tcp /proc/net/tcp6 2>/dev/null | awk '$4 == \"0A\"' | wc -l; \
         wc -l < /proc/net/tcp",
    );
    assert_eq!(
        out.split_whitespace().collect::<Vec<_>>(),
        ["0", "1"],
        "{out}"
    );
}

#[test]
fn stop_ends_a_process_that_left_the_group_and_ended_agrees() {
    let d = Data::new();
    let iso = d.iso();
    let c = iso.command(
        4,
        &d.ws,
        None,
        Path::new("/bin/sh"),
        &[
            "-c".into(),
            "setsid sleep 100 & echo up > up; exec sleep 100".into(),
        ],
        &[],
        None,
    );
    let mut cmd = std::process::Command::new(&c.program);
    cmd.args(&c.args).process_group(0).stdin(Stdio::null());
    let mut child = iso.spawn_with(4, &mut cmd, None).unwrap();
    assert!(wait_for(|| d.ws.join("up").exists(), 5));
    let info = iso.record(4).unwrap().unwrap();
    assert!(wait_for(|| in_namespace(info.pid_ns).len() >= 3, 5));
    // A new value, as after an engine restart, reads the record.
    let again = d.iso();
    assert!(!again.ended(4).unwrap());
    again.stop(4).unwrap();
    assert!(again.ended(4).unwrap());
    assert!(in_namespace(info.pid_ns).is_empty());
    let _ = child.wait();
    // A second stop finds nothing to do.
    again.stop(4).unwrap();
    assert!(iso.ended(99).unwrap(), "no record: ended");
    assert!(
        iso.name(4).ends_with("sessions/se-4.sandbox"),
        "{}",
        iso.name(4)
    );
}

#[test]
fn a_record_of_another_process_is_never_signaled() {
    let d = Data::new();
    let iso = d.iso();
    fs::create_dir_all(&iso.sessions).unwrap();
    // The identity of this test process: it agrees, so a record of it
    // counts as live (a bad record keeps a session live, never ended).
    let me = std::process::id() as i32;
    let (_, id) = identify(me).unwrap().unwrap();
    let rec = |boot: &str, start: u64, ns: u64| format!("{boot} {me} {start} {ns}\n");
    fs::write(iso.record_path(5), rec(&id.boot, id.start, id.pid_ns)).unwrap();
    assert!(!iso.ended(5).unwrap());
    // One value differs: the init of the record ended, and the process
    // with its ID is another one. No signal (the test is still alive).
    let other_boot = "00000000-0000-0000-0000-000000000000";
    for (n, text) in [
        (6, rec(&id.boot, id.start + 1, id.pid_ns)),
        (7, rec(&id.boot, id.start, id.pid_ns + 1)),
        (8, rec(other_boot, id.start, id.pid_ns)),
    ] {
        fs::write(iso.record_path(n), text).unwrap();
        assert!(iso.ended(n).unwrap(), "record {n}");
        iso.stop(n).unwrap();
    }
    for (n, bad) in [
        (9, "b x 1 1\n"),
        (10, &rec(&id.boot, 0, 1).replace(&format!(" {me} "), " 1 ")),
        (11, "a 7 1\n"),
    ] {
        fs::write(iso.record_path(n), bad).unwrap();
        assert!(
            iso.ended(n).is_err() && iso.stop(n).is_err(),
            "record {n}: {bad}"
        );
    }
}

#[test]
fn the_start_time_is_field_22_also_after_a_strange_name() {
    let tail = " S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 777 19 20";
    assert_eq!(start_time(&format!("42 (sh){tail}")), Some(777));
    assert_eq!(start_time(&format!("42 (a) b) (c d){tail}")), Some(777));
    assert_eq!(start_time("42 (sh) S 1"), None);
    assert_eq!(start_time("no paren"), None);
    // Property: each name, also with ")" and spaces, gives field 22.
    let mut x: u32 = 7;
    for _ in 0..2000 {
        x = x.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        let name: String = (0..(x % 16))
            .map(|i| [' ', ')', '(', 'a', '9'][((x >> (i % 16)) % 5) as usize])
            .collect();
        assert_eq!(
            start_time(&format!("42 ({name}){tail}")),
            Some(777),
            "{name}"
        );
    }
}

#[test]
fn a_failed_record_stops_the_sandbox_before_the_program_runs() {
    let d = Data::new();
    let iso = d.iso();
    // `sessions` is a file, so the record cannot be written.
    fs::write(&iso.sessions, "").unwrap();
    let c = iso.command(
        9,
        &d.ws,
        None,
        Path::new("/bin/sh"),
        &["-c".into(), "echo ran > ran".into()],
        &[],
        None,
    );
    let mut cmd = std::process::Command::new(&c.program);
    cmd.args(&c.args).process_group(0).stdin(Stdio::null());
    assert!(iso.spawn_with(9, &mut cmd, None).is_err());
    thread::sleep(Duration::from_millis(300));
    assert!(!d.ws.join("ran").exists());
}

/// The helper of the lifetime test: started by the test below as a second
/// process, it starts a sandbox and then waits to be killed.
#[test]
#[ignore = "run by the_kill_of_the_helper_ends_each_process"]
fn lifetime_helper() {
    let Some(dir) = std::env::var_os("IRIS_SBX_HELPER_DATA") else {
        return;
    };
    let dir = PathBuf::from(dir);
    let iso = Bwrap::new(&dir).unwrap();
    let c = iso.command(
        10,
        &dir.join("workspaces/j-1"),
        None,
        Path::new("/bin/sh"),
        &[
            "-c".into(),
            "setsid sleep 100 & echo up > up; exec sleep 100".into(),
        ],
        &[],
        None,
    );
    let mut cmd = std::process::Command::new(&c.program);
    cmd.args(&c.args).process_group(0).stdin(Stdio::null());
    let mut child = iso.spawn_with(10, &mut cmd, None).unwrap();
    println!("spawned");
    // The test kills this process before the wait ends.
    thread::sleep(Duration::from_secs(60));
    let _ = child.kill();
    let _ = child.wait();
}

#[test]
fn the_kill_of_the_helper_ends_each_process() {
    let d = Data::new();
    let mut helper = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "isolation::linux::tests::lifetime_helper",
            "--nocapture",
        ])
        .env("IRIS_SBX_HELPER_DATA", &d.dir)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = io::BufReader::new(helper.stdout.take().unwrap()).lines();
    assert!(lines.any(|l| l.map(|l| l.contains("spawned")).unwrap_or(false)));
    assert!(wait_for(|| d.ws.join("up").exists(), 5));
    let info = d.iso().record(10).unwrap().unwrap();
    assert!(wait_for(|| in_namespace(info.pid_ns).len() >= 3, 5));
    // SIGKILL: the helper runs no code after it.
    helper.kill().unwrap();
    helper.wait().unwrap();
    assert!(
        wait_for(|| in_namespace(info.pid_ns).is_empty(), 5),
        "processes of the session outlived the helper"
    );
    assert!(d.iso().ended(10).unwrap());
}

#[test]
fn identify_refuses_a_process_that_exited() {
    let mut c = std::process::Command::new("true").spawn().unwrap();
    // Exited, not reaped: a zombie still has its process ID.
    thread::sleep(Duration::from_millis(200));
    assert!(identify(c.id() as i32).unwrap().is_none());
    c.wait().unwrap();
    let mut s = std::process::Command::new("sleep")
        .arg("5")
        .spawn()
        .unwrap();
    let own = fs::metadata("/proc/self/ns/pid").unwrap().ino();
    assert_eq!(identify(s.id() as i32).unwrap().unwrap().1.pid_ns, own);
    s.kill().unwrap();
    s.wait().unwrap();
}

#[test]
fn only_the_program_file_is_in_the_view() {
    let d = Data::new();
    // A program in a folder of the person, beside a secret file.
    let folder = d.dir.with_extension("tool");
    fs::create_dir_all(&folder).unwrap();
    fs::write(folder.join("secret"), "HOME-MARKER").unwrap();
    let tool = folder.join("my-tool");
    fs::write(&tool, "#!/bin/sh\ncat \"$0\" >/dev/null && echo tool-ran; cat \"$(dirname \"$0\")/secret\" 2>/dev/null; ls \"$(dirname \"$0\")\"\n").unwrap();
    fs::set_permissions(&tool, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    let out = run(&d, 11, None, tool.to_str().unwrap(), &[]);
    let _ = fs::remove_dir_all(&folder);
    assert!(out.contains("tool-ran"), "{out}");
    assert!(!out.contains("HOME-MARKER"), "{out}");
    assert_eq!(out.lines().last(), Some("my-tool"), "{out}");
}

#[test]
fn the_relay_closes_a_ninth_connection() {
    // The egress side keeps each connection open.
    let egress = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = egress.local_addr().unwrap().port();
    let held = Arc::new(std::sync::Mutex::new(Vec::new()));
    let h = held.clone();
    thread::spawn(move || {
        for s in egress.incoming().flatten() {
            h.lock().unwrap().push(s);
        }
    });
    let inner = TcpListener::bind("127.0.0.1:0").unwrap();
    let inner_port = inner.local_addr().unwrap().port();
    let engine = pidfd_open(std::process::id() as i32).unwrap();
    let from = crate::egress::session_peer(3).unwrap();
    start_relay(inner, port, from, engine).unwrap();
    let mut open = Vec::new();
    for _ in 0..RELAY_MAX {
        let s = TcpStream::connect(("127.0.0.1", inner_port)).unwrap();
        open.push(s);
    }
    assert!(wait_for(|| held.lock().unwrap().len() == RELAY_MAX, 5));
    let mut ninth = TcpStream::connect(("127.0.0.1", inner_port)).unwrap();
    ninth
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut b = [0u8; 1];
    assert_eq!(ninth.read(&mut b).unwrap(), 0, "the ninth is closed");
    assert_eq!(held.lock().unwrap().len(), RELAY_MAX);
    // Bytes pass in both directions on an open connection.
    open[0].write_all(b"a").unwrap();
    let mut far = held.lock().unwrap().remove(0);
    // Each connection comes from the session address.
    assert_eq!(far.peer_addr().unwrap().ip(), std::net::IpAddr::V4(from));
    let mut got = [0u8; 1];
    far.read_exact(&mut got).unwrap();
    far.write_all(b"b").unwrap();
    open[0].read_exact(&mut got).unwrap();
    assert_eq!(&got, b"b");
}

#[test]
fn the_relay_stops_at_the_end_of_its_engine() {
    let egress = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = egress.local_addr().unwrap().port();
    egress.set_nonblocking(true).unwrap();
    // A process in the place of the engine.
    let mut fake = std::process::Command::new("sleep")
        .arg("30")
        .spawn()
        .unwrap();
    let engine = pidfd_open(fake.id() as i32).unwrap();
    let inner = TcpListener::bind("127.0.0.1:0").unwrap();
    let inner_port = inner.local_addr().unwrap().port();
    let from = crate::egress::session_peer(4).unwrap();
    start_relay(inner, port, from, engine).unwrap();
    // While the engine runs, a connection goes through.
    let _a = TcpStream::connect(("127.0.0.1", inner_port)).unwrap();
    assert!(wait_for(|| egress.accept().is_ok(), 5));
    fake.kill().unwrap();
    fake.wait().unwrap();
    // After its end: no new connection reaches the port, and the relay
    // closes its listener.
    if let Ok(mut b) = TcpStream::connect(("127.0.0.1", inner_port)) {
        b.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        let mut x = [0u8; 1];
        assert_eq!(b.read(&mut x).unwrap_or(0), 0);
    }
    thread::sleep(Duration::from_millis(200));
    assert!(
        egress.accept().is_err(),
        "a connection after the end of the engine"
    );
    assert!(wait_for(
        || TcpStream::connect(("127.0.0.1", inner_port)).is_err(),
        5
    ));
}

#[test]
fn the_info_parser_takes_only_a_valid_line() {
    let ok = br#"{
    "child-pid": 2355,
    "cgroup-namespace": 4026532269,
    "pid-namespace": 4026532268
}
"#;
    assert_eq!(parse_info(ok), Some(2355));
    assert_eq!(
        parse_info(br#"{"child-pid": 7}"#),
        Some(7),
        "other fields are not needed"
    );
    for bad in [
        &br#"{"child-pid": 1, "pid-namespace": 5}"#[..],
        br#"{"child-pid": -3, "pid-namespace": 5}"#,
        br#"{"child-pid": 99999999999, "pid-namespace": 5}"#,
        br#"{"pid-namespace": 5}"#,
        br#"{"child-pid": "7", "pid-namespace": 5}"#,
        b"",
        b"}",
        b"{",
    ] {
        assert_eq!(parse_info(bad), None, "{}", String::from_utf8_lossy(bad));
    }
    // Property: random bytes, and random cuts of a valid line, never panic
    // and never give an init of 0 or 1.
    let mut x: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut next = || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    for _ in 0..20_000 {
        let len = (next() % 80) as usize;
        let mut v: Vec<u8> = if next() % 2 == 0 {
            (0..len).map(|_| next() as u8).collect()
        } else {
            ok[..len.min(ok.len())].to_vec()
        };
        if next() % 3 == 0 {
            v.push(b'}');
        }
        if let Some(pid) = parse_info(&v) {
            assert!(pid > 1);
        }
    }
}

/// After an engine crash, the port of session 7 can go to the proxy of
/// session 8. The relay of session 7 then reaches that proxy, which refuses
/// it: no timing makes the relay reach another session.
#[test]
fn a_relay_reaches_no_proxy_of_another_session() {
    use crate::egress::{session_peer, Proxy, SystemResolver};
    let log = std::env::temp_dir().join(format!("iris-sbx-peer-{}.log", std::process::id()));
    let _ = fs::remove_file(&log);
    let file = OpenOptions::new()
        .create_new(true)
        .append(true)
        .open(&log)
        .unwrap();
    let other = Proxy::start(
        vec!["api.example.test".into()],
        file,
        Box::new(SystemResolver),
        session_peer(8).unwrap(),
        None,
    )
    .unwrap();
    let inner = TcpListener::bind("127.0.0.1:0").unwrap();
    let inner_port = inner.local_addr().unwrap().port();
    let engine = pidfd_open(std::process::id() as i32).unwrap();
    start_relay(inner, other.port(), session_peer(7).unwrap(), engine).unwrap();
    let mut s = TcpStream::connect(("127.0.0.1", inner_port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(b"CONNECT api.example.test:443 HTTP/1.1\r\n\r\n")
        .unwrap();
    let mut line = String::new();
    std::io::BufReader::new(s).read_line(&mut line).unwrap();
    assert!(line.starts_with("HTTP/1.1 403"), "{line}");
    other.stop().unwrap();
    let text = fs::read_to_string(&log).unwrap();
    let _ = fs::remove_file(&log);
    assert!(text.contains("refused reason=not_session"), "{text}");
}

/// A data folder that is a link: each mount of the view uses the real
/// path, so the real folder is the one that the empty mount hides. A data
/// folder that cannot be resolved gives no isolation.
#[test]
fn a_linked_data_folder_is_hidden_at_its_real_path() {
    let d = Data::new();
    let link = d.dir.with_extension("link");
    let _ = fs::remove_file(&link);
    std::os::unix::fs::symlink(&d.dir, &link).unwrap();
    let real = fs::canonicalize(&d.dir).unwrap();
    let iso = Bwrap::new(&link).unwrap();
    assert_eq!(iso.data_dir, real);
    let ws_link = link.join("workspaces/j-1");
    let c = iso.command(9, &ws_link, None, Path::new("/bin/true"), &[], &[], None);
    let args: Vec<&OsStr> = c.args.iter().map(|a| a.as_os_str()).collect();
    let after = |flag: &str| -> Vec<&OsStr> {
        args.windows(2)
            .filter(|w| w[0] == OsStr::new(flag))
            .map(|w| w[1])
            .collect()
    };
    assert!(after("--tmpfs").contains(&real.as_os_str()), "{args:?}");
    assert!(!args.contains(&link.as_os_str()), "{args:?}");
    assert!(!args.contains(&ws_link.as_os_str()), "{args:?}");
    assert!(after("--chdir").contains(&real.join("workspaces/j-1").as_os_str()));
    // The sandbox through the link also hides the key.
    let out = {
        let args: Vec<String> = vec![
            "-c".into(),
            format!(
                "cat {}/key {}/key 2>/dev/null; echo end",
                link.display(),
                real.display()
            ),
        ];
        let c = iso.command(9, &ws_link, None, Path::new("/bin/sh"), &args, &[], None);
        let mut cmd = std::process::Command::new(&c.program);
        cmd.args(&c.args)
            .process_group(0)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let child = iso.spawn_with(9, &mut cmd, None).unwrap();
        String::from_utf8_lossy(&child.wait_with_output().unwrap().stdout).into_owned()
    };
    assert!(out.contains("end") && !out.contains("KEY-MARKER"), "{out}");
    fs::remove_file(&link).unwrap();
    assert!(Bwrap::new(&link).is_err());
}

/// A HOME with a link in its path: the private home is at the real path,
/// and HOME in the sandbox names it, so the agent can write there.
#[test]
fn a_linked_home_is_usable() {
    let d = Data::new();
    let real = d.dir.with_extension("home-real");
    let link = d.dir.with_extension("home-link");
    let _ = fs::remove_dir_all(&real);
    let _ = fs::remove_file(&link);
    fs::create_dir_all(real.join("user")).unwrap();
    std::os::unix::fs::symlink(&real, &link).unwrap();
    let iso = d.iso();
    let mut args = iso.view(Some(&link.join("user")), &[], &[], &d.ws);
    args.extend([
        "--".into(),
        "/bin/sh".into(),
        "-c".into(),
        "echo x > \"$HOME/f\" && cat \"$HOME/f\" && echo \"$HOME\"".into(),
    ]);
    let out = std::process::Command::new(&iso.bwrap)
        .args(&args)
        .env("HOME", link.join("user"))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    let real_home = fs::canonicalize(real.join("user")).unwrap();
    assert!(
        out.status.success(),
        "{text} {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(text, format!("x\n{}\n", real_home.display()));
    // The private home is empty: nothing reached the host folder.
    assert!(!real.join("user/f").exists());
    let _ = fs::remove_file(&link);
    let _ = fs::remove_dir_all(&real);
}

/// With no HOME, and with a TMPDIR outside the view, the agent still gets
/// a private home and a private temporary folder that it can write.
#[test]
fn the_agent_can_always_write_its_home_and_tmpdir() {
    let d = Data::new();
    let iso = d.iso();
    for home in [None, Some(Path::new("/")), Some(Path::new("relative"))] {
        let mut args = iso.view(home, &[], &[], &d.ws);
        args.extend([
            "--".into(),
            "/bin/sh".into(),
            "-c".into(),
            "echo h > \"$HOME/f\" && echo t > \"$TMPDIR/f\" && echo \"$HOME $TMPDIR\"".into(),
        ]);
        let out = std::process::Command::new(&iso.bwrap)
            .args(&args)
            .env_remove("HOME")
            .env("TMPDIR", "/var/tmp")
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success(),
            "{home:?}: {text} {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(text, "/home/iris /tmp\n", "{home:?}");
    }
}

/// A read of `/proc/<pid>` of a process that ends during the read fails
/// with `ESRCH` (seen as `No such process` in a stop), not only with
/// `NotFound`. Both mean that the process ended.
#[test]
fn an_error_of_an_ended_process_means_ended() {
    assert!(exited(&io::Error::from_raw_os_error(libc::ESRCH)));
    assert!(exited(&io::Error::from(io::ErrorKind::NotFound)));
    assert!(!exited(&io::Error::from_raw_os_error(libc::EACCES)));
}

/// A tool folder outside `/tmp` (a folder of system state), for the read
/// path tests.
fn tool_dir(name: &str) -> PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/read-paths-tests")
        .join(format!("sbx-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    fs::write(p.join("main.js"), "TOOL-MARKER").unwrap();
    fs::canonicalize(&p).unwrap()
}

#[test]
fn a_read_path_is_readable_and_not_writable() {
    let d = Data::new();
    let iso = d.iso();
    let tool = tool_dir("read");
    let script = format!(
        "cat {t}/main.js; echo; echo x > {t}/new && echo WROTE; cat {k} 2>/dev/null",
        t = tool.display(),
        k = d.dir.join("key").display()
    );
    let c = iso.command(
        21,
        &d.ws,
        None,
        Path::new("/bin/sh"),
        &["-c".into(), script],
        std::slice::from_ref(&tool),
        None,
    );
    assert!(c.args.iter().any(|a| a == RO_BIND_FD));
    let mut cmd = std::process::Command::new(&c.program);
    cmd.args(&c.args)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let out = iso
        .spawn_with(21, &mut cmd, None)
        .unwrap()
        .wait_with_output()
        .unwrap();
    let out = String::from_utf8_lossy(&out.stdout);
    assert!(out.contains("TOOL-MARKER"), "{out}");
    assert!(!out.contains("WROTE"), "{out}");
    assert!(!out.contains("KEY-MARKER"), "{out}");
    assert!(!tool.join("new").exists());
    let _ = fs::remove_dir_all(&tool);
}

#[test]
fn a_read_path_that_changes_after_the_launch_stops_the_start() {
    let d = Data::new();
    let iso = d.iso();
    let tool = tool_dir("swap");
    let c = iso.command(
        22,
        &d.ws,
        None,
        Path::new("/bin/sh"),
        &["-c".into(), "echo ran > ran".into()],
        std::slice::from_ref(&tool),
        None,
    );
    // A process of the person puts a link to the data folder in its place.
    let old = tool.with_extension("old");
    fs::rename(&tool, &old).unwrap();
    std::os::unix::fs::symlink(&d.dir, &tool).unwrap();
    let mut cmd = std::process::Command::new(&c.program);
    cmd.args(&c.args).process_group(0).stdin(Stdio::null());
    let e = iso.spawn_with(22, &mut cmd, None).unwrap_err();
    assert!(e.to_string().contains("read path"), "{e}");
    thread::sleep(Duration::from_millis(200));
    assert!(!d.ws.join("ran").exists());
    let _ = fs::remove_file(&tool);
    let _ = fs::remove_dir_all(&old);
}

#[test]
fn a_read_path_replaced_by_an_unsafe_folder_stops_the_start() {
    let d = Data::new();
    let iso = d.iso();
    let tool = tool_dir("swap-dir");
    let c = iso.command(
        23,
        &d.ws,
        None,
        Path::new("/bin/sh"),
        &["-c".into(), "echo ran > ran".into()],
        std::slice::from_ref(&tool),
        None,
    );
    // A plain folder takes the path, with a socket in it: the helper checks
    // the new folder again, not only its name.
    let old = tool.with_extension("old");
    fs::rename(&tool, &old).unwrap();
    fs::create_dir(&tool).unwrap();
    let _l = std::os::unix::net::UnixListener::bind(tool.join("agent.sock")).unwrap();
    let mut cmd = std::process::Command::new(&c.program);
    cmd.args(&c.args).process_group(0).stdin(Stdio::null());
    let e = iso.spawn_with(23, &mut cmd, None).unwrap_err();
    assert!(e.to_string().contains("socket"), "{e}");
    thread::sleep(Duration::from_millis(200));
    assert!(!d.ws.join("ran").exists());
    let _ = fs::remove_dir_all(&tool);
    let _ = fs::remove_dir_all(&old);
}

#[test]
fn the_local_port_is_reached_and_no_other_local_port() {
    let d = Data::new();
    let local = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = local.local_addr().unwrap().port();
    let other = TcpListener::bind("127.0.0.1:0").unwrap();
    let other_port = other.local_addr().unwrap().port();
    let got = thread::spawn(move || {
        let (mut s, peer) = local.accept().unwrap();
        let mut b = [0u8; 2];
        s.read_exact(&mut b).unwrap();
        s.write_all(b"ok").unwrap();
        (b, peer.ip())
    });
    let script = format!(
        "import os, socket\n\
         print('var', os.environ.get('{var}'))\n\
         s = socket.create_connection(('127.0.0.1', {port}), timeout=5)\n\
         s.sendall(b'hi'); print('local', s.recv(2).decode())\n\
         try:\n\
         \x20   socket.create_connection(('127.0.0.1', {other_port}), timeout=2); print('REACHED')\n\
         except OSError: print('blocked')\n",
        var = crate::LOCAL_PORT_VAR
    );
    let iso = d.iso();
    let args = vec!["-c".to_string(), script];
    let c = iso.command(
        41,
        &d.ws,
        None,
        Path::new("/usr/bin/python3"),
        &args,
        &[],
        Some(port),
    );
    let mut cmd = std::process::Command::new(&c.program);
    cmd.args(&c.args)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    let child = iso.spawn_with(41, &mut cmd, None).unwrap();
    let out = String::from_utf8_lossy(&child.wait_with_output().unwrap().stdout).into_owned();
    let (bytes, peer) = got.join().unwrap();
    assert_eq!(&bytes, b"hi");
    // The relay connects from 127.0.0.1, not from a session address.
    assert_eq!(peer, std::net::IpAddr::from(Ipv4Addr::LOCALHOST));
    assert!(out.contains(&format!("var {port}")), "{out}");
    assert!(out.contains("local ok"), "{out}");
    assert!(out.contains("blocked") && !out.contains("REACHED"), "{out}");
    drop(other);
}

#[test]
fn a_local_port_pair_after_the_program_is_not_read() {
    let mut c = std::process::Command::new("bwrap");
    c.args(["--", "/bin/t", "--setenv", crate::LOCAL_PORT_VAR, "22"]);
    assert_eq!(local_port_of(&c), None);
    let mut c = std::process::Command::new("bwrap");
    c.args(["--setenv", crate::LOCAL_PORT_VAR, "11434", "--", "/bin/t"]);
    assert_eq!(local_port_of(&c), Some(11434));
}
