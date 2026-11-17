#!/usr/bin/env python3
"""Probe for card R4-05m (macOS sandbox). Runs on macOS only.

It answers two questions with evidence:

1. Lifetime: can a process of a session escape the stop? The probe starts a
   helper, and the helper starts processes that call setsid and fork-and-exit
   in a loop (the escape that stopped the first Mac design). It compares a
   plain child (baseline) with a helper that runs as a launchd job, and reads
   the resource coalition of each process.
2. Containment: does a Seatbelt (sandbox-exec) profile with "deny default"
   block files, network, Unix sockets, launchd jobs, the pasteboard and
   signals, and still let a program run?

Each process of the probe ends by itself before its deadline (DEADLINE_S).
At the end the probe kills each process that carries its marker, and says
how many it found. It reads no file content of the person and records no
pasteboard content (only a length).

Usage: python3 scripts/probes/macos-sandbox-probe.py
"""

import ctypes
import json
import os
import plistlib
import signal
import socket
import subprocess
import sys
import tempfile
import time

DEADLINE_S = 20.0
PROC_PIDCOALITIONINFO = 20
ROUNDS = 5


# --- process facts -------------------------------------------------------

_libproc = None


def libproc():
    global _libproc
    if _libproc is None:
        _libproc = ctypes.CDLL("/usr/lib/libproc.dylib")
    return _libproc


def coalition(pid):
    """The (resource, jetsam) coalition IDs of pid, or None."""
    buf = (ctypes.c_uint64 * 5)()
    n = libproc().proc_pidinfo(pid, PROC_PIDCOALITIONINFO, ctypes.c_uint64(0), buf, ctypes.sizeof(buf))
    if n <= 0:
        return None
    return (buf[0], buf[1])


def coalition_counts(cid):
    """(tasks_started, tasks_exited) of a resource coalition, or an error."""
    buf = (ctypes.c_uint64 * 128)()
    f = ctypes.CDLL(None, use_errno=True).coalition_info_resource_usage
    f.argtypes = [ctypes.c_uint64, ctypes.c_void_p, ctypes.c_size_t]
    r = f(cid, buf, ctypes.sizeof(buf))
    if r != 0:
        return "error rc=%d errno=%d" % (r, ctypes.get_errno())
    return (buf[0], buf[1])


def all_pids():
    lp = libproc()
    n = lp.proc_listallpids(None, 0)
    buf = (ctypes.c_int * (n + 256))()
    n = lp.proc_listallpids(buf, ctypes.sizeof(buf))
    return [buf[i] for i in range(n) if buf[i] > 0]


def pids_in_coalition(cid):
    out = []
    for pid in all_pids():
        c = coalition(pid)
        if c and c[0] == cid:
            out.append(pid)
    return out


def marker_pids(marker):
    r = subprocess.run(["/usr/bin/pgrep", "-f", marker], capture_output=True, text=True)
    me = os.getpid()
    return [int(p) for p in r.stdout.split() if int(p) != me]


# --- the processes of a fake session ---------------------------------------


def beat(fd, n):
    os.pwrite(fd, ("%d %.3f %d     \n" % (os.getpid(), time.time(), n)).encode(), 0)


def run_hopper(outdir, deadline, marker):
    """setsid, then fork and let the parent exit, again and again."""
    del marker
    if os.fork():
        os._exit(0)
    os.setsid()
    fd = os.open(os.path.join(outdir, "hop.beat"), os.O_WRONLY | os.O_CREAT, 0o600)
    n, last = 0, 0.0
    while time.time() < deadline:
        pid = os.fork()
        if pid:
            os._exit(0)
        n += 1
        now = time.time()
        if now - last > 0.05:
            beat(fd, n)
            last = now
    os._exit(0)


def run_sleeper(outdir, deadline, marker):
    """A daemon: double fork and setsid, then live until the deadline."""
    del marker
    if os.fork():
        os._exit(0)
    os.setsid()
    if os.fork():
        os._exit(0)
    fd = os.open(os.path.join(outdir, "sleep.beat"), os.O_WRONLY | os.O_CREAT, 0o600)
    n = 0
    while time.time() < deadline:
        n += 1
        beat(fd, n)
        time.sleep(0.1)
    os._exit(0)


def run_helper(outdir, deadline, marker, profile):
    """The helper of a session: start the two escapers, then wait."""
    with open(os.path.join(outdir, "helper.json"), "w") as f:
        json.dump({"pid": os.getpid(), "coalition": coalition(os.getpid())}, f)
    me = [sys.executable, os.path.abspath(__file__)]
    for mode in ("hopper", "sleeper"):
        args = me + [mode, outdir, str(deadline), marker]
        if profile:
            args = ["/usr/bin/sandbox-exec", "-f", profile] + args
        subprocess.Popen(args, start_new_session=True)
    while time.time() < deadline:
        time.sleep(0.2)


def fresh(outdir, name, window=1.0):
    """True if the beat file was written in the last `window` seconds."""
    try:
        with open(os.path.join(outdir, name)) as f:
            parts = f.read().split()
        return time.time() - float(parts[1]) < window, int(parts[0])
    except (OSError, IndexError, ValueError):
        return False, None


def escapers_alive(outdir):
    hop, hop_pid = fresh(outdir, "hop.beat")
    slp, slp_pid = fresh(outdir, "sleep.beat")
    return {"hopper_alive": hop, "sleeper_alive": slp, "hopper_pid": hop_pid, "sleeper_pid": slp_pid}


# --- lifetime tests --------------------------------------------------------


def test_baseline(marker, profile):
    """A plain child helper, killed with SIGKILL."""
    outdir = tempfile.mkdtemp(prefix="iris-probe-")
    deadline = time.time() + DEADLINE_S
    p = subprocess.Popen([sys.executable, os.path.abspath(__file__), "helper", outdir, str(deadline), marker, profile or ""])
    time.sleep(1.5)
    before = escapers_alive(outdir)
    p.kill()
    p.wait()
    time.sleep(3)
    return {"before_kill": before, "after_kill": escapers_alive(outdir)}


def uid_domain():
    return "gui/%d" % os.getuid()


def start_job(label, outdir, deadline, marker, profile):
    plist = os.path.join(outdir, label + ".plist")
    with open(plist, "wb") as f:
        plistlib.dump(
            {
                "Label": label,
                "ProgramArguments": [sys.executable, os.path.abspath(__file__), "helper", outdir, str(deadline), marker, profile or ""],
                "RunAtLoad": True,
                "KeepAlive": False,
                "StandardOutPath": os.path.join(outdir, "helper.log"),
                "StandardErrorPath": os.path.join(outdir, "helper.log"),
            },
            f,
        )
    t0 = time.time()
    r = subprocess.run(["/bin/launchctl", "bootstrap", uid_domain(), plist], capture_output=True, text=True)
    if r.returncode != 0:
        return None, "bootstrap rc=%d %s" % (r.returncode, r.stderr.strip())
    for _ in range(100):
        if os.path.exists(os.path.join(outdir, "helper.json")):
            break
        time.sleep(0.02)
    started = time.time() - t0
    try:
        with open(os.path.join(outdir, "helper.json")) as f:
            helper = json.load(f)
    except OSError:
        return None, "helper did not start"
    helper["start_seconds"] = round(started, 3)
    pr = subprocess.run(["/bin/launchctl", "print", "%s/%s" % (uid_domain(), label)], capture_output=True, text=True)
    helper["launchctl_print_coalition_lines"] = [l.strip() for l in pr.stdout.splitlines() if "coalition" in l.lower()]
    return helper, None


def bootout(label):
    t0 = time.time()
    r = subprocess.run(["/bin/launchctl", "bootout", "%s/%s" % (uid_domain(), label)], capture_output=True, text=True)
    return {"rc": r.returncode, "stderr": r.stderr.strip()[:200], "seconds": round(time.time() - t0, 3)}


def test_launchd(marker, profile, how, n):
    """A helper that is a launchd job. how = 'bootout' or 'sigkill'."""
    outdir = tempfile.mkdtemp(prefix="iris-probe-")
    label = "com.iris.probe.%s.%d.%d" % (how, os.getpid(), n)
    deadline = time.time() + DEADLINE_S
    helper, err = start_job(label, outdir, deadline, marker, profile)
    if err:
        return {"error": err}
    time.sleep(1.5)
    res = {"helper": helper, "probe_coalition": coalition(os.getpid()), "before": escapers_alive(outdir)}
    for key in ("hopper_pid", "sleeper_pid"):
        pid = res["before"].get(key)
        res["before"][key.replace("pid", "coalition")] = coalition(pid) if pid else None
    cid = helper["coalition"][0] if helper.get("coalition") else None
    res["coalition_members_before"] = len(pids_in_coalition(cid)) if cid else None
    if how == "sigkill":
        os.kill(helper["pid"], signal.SIGKILL)
        time.sleep(3)
        res["after"] = escapers_alive(outdir)
        res["coalition_members_after"] = len(pids_in_coalition(cid)) if cid else None
        res["bootout"] = bootout(label)
    else:
        res["bootout"] = bootout(label)
        time.sleep(3)
        res["after"] = escapers_alive(outdir)
        res["coalition_members_after"] = len(pids_in_coalition(cid)) if cid else None
    return res


def kill_loop(cid, limit=5.0):
    """Stop then kill each member of the coalition until a scan and the
    counters say it is empty. Gives the pass count and the result."""
    t0, passes, empty_scans = time.time(), 0, 0
    while time.time() - t0 < limit:
        passes += 1
        members = pids_in_coalition(cid)
        for sig in (signal.SIGSTOP, signal.SIGKILL):
            for pid in members:
                try:
                    os.kill(pid, sig)
                except OSError:
                    pass
        c = coalition_counts(cid)
        if not members and (not isinstance(c, tuple) or c[0] == c[1]):
            break  # ESRCH: the coalition is gone, so it is empty
        empty_scans = empty_scans + 1 if not members else 0
        time.sleep(0.01)
    return {"passes": passes, "seconds": round(time.time() - t0, 3), "counts_end": coalition_counts(cid),
            "empty_scans_at_end": empty_scans}


def test_kill_loop(marker, profile, n):
    """A launchd helper with escapers; bootout, then the coalition kill loop."""
    outdir = tempfile.mkdtemp(prefix="iris-probe-")
    label = "com.iris.probe.loop.%d.%d" % (os.getpid(), n)
    deadline = time.time() + DEADLINE_S
    helper, err = start_job(label, outdir, deadline, marker, profile)
    if err:
        return {"error": err}
    time.sleep(1.5)
    cid = helper["coalition"][0]
    res = {"before": escapers_alive(outdir), "counts_before": coalition_counts(cid)}
    res["bootout"] = bootout(label)
    time.sleep(0.5)
    res["after_bootout"] = escapers_alive(outdir)
    res["single_scan_kill"] = None
    members = pids_in_coalition(cid)
    for pid in members:
        try:
            os.kill(pid, signal.SIGKILL)
        except OSError:
            pass
    time.sleep(1.5)
    res["after_single_scan_kill"] = escapers_alive(outdir)
    res["counts_after_single_scan_kill"] = coalition_counts(cid)
    res["loop"] = kill_loop(cid)
    time.sleep(1.5)
    res["after_loop"] = escapers_alive(outdir)
    res["counts_after_loop"] = coalition_counts(cid)
    return res


def run_init(outdir, deadline, marker):
    """The first process of a sandbox: fork the escapers (same sandbox), then
    on SIGUSR1 call kill(-1, SIGKILL) in the non-POSIX form (the caller is
    not a target) until no process is left to signal."""
    import errno
    go = []
    signal.signal(signal.SIGUSR1, lambda *_: go.append(1))
    with open(os.path.join(outdir, "init.json"), "w") as f:
        json.dump({"pid": os.getpid(), "coalition": coalition(os.getpid())}, f)
    for fn in (run_hopper, run_sleeper):
        if os.fork() == 0:
            fn(outdir, deadline, marker)
    libc = ctypes.CDLL(None, use_errno=True)
    # Safety check first: SIGURG (default action: ignore). The probe outside
    # the sandbox counts it. The SIGKILL loop runs only if the probe says
    # that it got none (SIGUSR1).
    r = libc.syscall(37, -1, signal.SIGURG, 0)
    with open(os.path.join(outdir, "urg.json"), "w") as f:
        json.dump({"rc": r, "errno": ctypes.get_errno()}, f)
    while not go and time.time() < deadline:
        time.sleep(0.05)
    t0, passes, signaled, none_in_row = time.time(), 0, 0, 0
    while time.time() - t0 < 5 and none_in_row < 3:
        passes += 1
        r = libc.syscall(37, -1, signal.SIGKILL, 0)  # SYS_kill, posix = 0
        if r == 0:
            signaled += 1
            none_in_row = 0
        elif ctypes.get_errno() == errno.ESRCH:
            none_in_row += 1
        else:
            none_in_row = 0
        while True:  # reap the children of init
            try:
                if os.waitpid(-1, os.WNOHANG)[0] == 0:
                    break
            except ChildProcessError:
                break
    with open(os.path.join(outdir, "killer.json"), "w") as f:
        json.dump({"passes": passes, "passes_that_signaled": signaled, "seconds": round(time.time() - t0, 3),
                   "last_errno": ctypes.get_errno()}, f)
    os._exit(0)


URG = []


def test_init_killer(marker, profile, n):
    """A launchd job whose program is a sandboxed init with the escapers."""
    outdir = tempfile.mkdtemp(prefix="iris-probe-")
    label = "com.iris.probe.init.%d.%d" % (os.getpid(), n)
    deadline = time.time() + DEADLINE_S
    plist = os.path.join(outdir, label + ".plist")
    with open(plist, "wb") as f:
        plistlib.dump({"Label": label, "RunAtLoad": True, "KeepAlive": False,
                       "ProgramArguments": ["/usr/bin/sandbox-exec", "-f", profile, sys.executable,
                                            os.path.abspath(__file__), "init", outdir, str(deadline), marker],
                       "StandardOutPath": os.path.join(outdir, "init.log"),
                       "StandardErrorPath": os.path.join(outdir, "init.log")}, f)
    subprocess.run(["/bin/launchctl", "bootstrap", uid_domain(), plist], capture_output=True)
    for _ in range(150):
        if os.path.exists(os.path.join(outdir, "init.json")):
            break
        time.sleep(0.02)
    try:
        with open(os.path.join(outdir, "init.json")) as f:
            init = json.load(f)
    except OSError:
        log = open(os.path.join(outdir, "init.log")).read()[-400:] if os.path.exists(os.path.join(outdir, "init.log")) else ""
        bootout(label)
        return {"error": "init did not start", "log": log}
    time.sleep(1.5)
    cid = init["coalition"][0]
    res = {"before": escapers_alive(outdir), "counts_before": coalition_counts(cid)}
    try:
        res["urg"] = json.load(open(os.path.join(outdir, "urg.json")))
    except OSError:
        res["urg"] = None
    res["canary_got_urg"] = len(URG)
    if URG or res["urg"] is None:
        # kill(-1) reached a process outside the sandbox: never send SIGKILL.
        for pid in pids_in_coalition(cid):
            os.kill(pid, signal.SIGKILL)
        res["bootout"] = bootout(label)
        res["aborted"] = "kill(-1) is not limited to the sandbox"
        return res
    os.kill(init["pid"], signal.SIGUSR1)
    for _ in range(400):
        if os.path.exists(os.path.join(outdir, "killer.json")):
            break
        time.sleep(0.02)
    time.sleep(1.5)
    try:
        with open(os.path.join(outdir, "killer.json")) as f:
            res["killer"] = json.load(f)
    except OSError:
        res["killer"] = "no result"
    res["after"] = escapers_alive(outdir)
    res["counts_after"] = coalition_counts(cid)
    res["members_after"] = len(pids_in_coalition(cid))
    res["bootout"] = bootout(label)
    return res


# --- containment test ------------------------------------------------------


def listener():
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    s.listen(8)
    return s, s.getsockname()[1]


def write_profile(path, workspace, priv, port, read_extra):
    reads = " ".join('(subpath "%s")' % p for p in read_extra)
    text = f"""(version 1)
(deny default)
(allow process-exec)
(allow process-fork)
(allow signal (target same-sandbox))
(allow process-info* (target same-sandbox))
(allow sysctl-read)
(allow file-read-metadata)
(allow file-read*
  (literal "/")
  (subpath "/usr/lib") (subpath "/usr/bin") (subpath "/usr/share") (subpath "/bin")
  (subpath "/System") (subpath "/Library/Apple") (subpath "/private/var/db/dyld")
  (literal "/dev/null") (literal "/dev/urandom") (literal "/dev/random") (literal "/dev/zero")
  (subpath "/private/etc/ssl") (literal "/private/etc/hosts") (literal "/private/etc/passwd")
  {reads}
  (subpath "{workspace}") (subpath "{priv}"))
(allow file-write* (subpath "{workspace}") (subpath "{priv}") (literal "/dev/null"))
(allow network-outbound (remote ip "localhost:{port}"))
"""
    with open(path, "w") as f:
        f.write(text)


def test_containment(marker):
    base = os.path.realpath(tempfile.mkdtemp(prefix="iris-probe-"))
    data = os.path.join(base, "data")
    workspace = os.path.join(data, "jobs", "j-1")
    priv = os.path.join(base, "private")
    for d in (workspace, priv):
        os.makedirs(d)
    with open(os.path.join(data, "store.key"), "w") as f:
        f.write("not a secret\n")
    good, good_port = listener()
    bad, bad_port = listener()
    unix_path = os.path.join(base, "agent.sock")
    us = socket.socket(socket.AF_UNIX)
    us.bind(unix_path)
    us.listen(1)
    read_extra = {os.path.realpath(sys.base_prefix), os.path.dirname(os.path.realpath(sys.executable))}
    if os.path.isdir("/opt/homebrew"):
        read_extra.add("/opt/homebrew")
    profile = os.path.join(base, "session.sb")
    write_profile(profile, workspace, priv, good_port, sorted(read_extra))
    args = {
        "data": data, "workspace": workspace, "home": os.path.expanduser("~"),
        "good": good_port, "bad": bad_port, "unix": unix_path, "parent": os.getpid(),
        "marker": marker,
    }
    env = {"HOME": priv, "TMPDIR": priv, "PATH": "/usr/bin:/bin"}
    script = os.path.join(priv, "probe.py")
    with open(os.path.abspath(__file__)) as src, open(script, "w") as dst:
        dst.write(src.read())
    t0 = time.time()
    r = subprocess.run(
        ["/usr/bin/sandbox-exec", "-f", profile, os.path.realpath(sys.executable), script, "inside", json.dumps(args)],
        capture_output=True, text=True, env=env, timeout=60,
    )
    res = {"rc": r.returncode, "seconds": round(time.time() - t0, 3), "stderr": r.stderr.strip()[-600:]}
    try:
        with open(os.path.join(workspace, "result.json")) as f:
            res["checks"] = json.load(f)
    except OSError as e:
        res["checks"] = "no result: %s" % e
    escape_label = "com.iris.probe.escape." + marker
    pr = subprocess.run(["/bin/launchctl", "print", "%s/%s" % (uid_domain(), escape_label)], capture_output=True)
    res["escape_job_exists_outside"] = pr.returncode == 0
    if pr.returncode == 0:
        subprocess.run(["/bin/launchctl", "remove", escape_label])
    res["profile"] = open(profile).read()
    return res


def attempt(fn):
    try:
        v = fn()
        return {"ok": True, "value": v}
    except Exception as e:  # the probe records each failure as data
        return {"ok": False, "error": "%s: %s" % (type(e).__name__, e)}


def run_inside(argjson):
    a = json.loads(argjson)
    out = {}

    def connect(port):
        s = socket.create_connection(("127.0.0.1", port), timeout=2)
        s.close()
        return "connected"

    def public():
        s = socket.create_connection(("1.1.1.1", 443), timeout=3)
        s.close()
        return "connected"

    def udp():
        s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        return s.sendto(b"x", ("1.1.1.1", 53))

    def unix():
        s = socket.socket(socket.AF_UNIX)
        s.connect(a["unix"])
        return "connected"

    def write_outside():
        p = os.path.join(a["home"], ".iris-probe-" + a["marker"])
        with open(p, "w") as f:
            f.write("x")
        os.remove(p)
        return "written"

    def run(cmd):
        r = subprocess.run(cmd, capture_output=True, timeout=10)
        return {"rc": r.returncode, "stdout_len": len(r.stdout), "stderr": r.stderr.decode(errors="replace")[:200]}

    out["read_data_key"] = attempt(lambda: open(os.path.join(a["data"], "store.key")).read() and "read")
    out["list_data_dir"] = attempt(lambda: len(os.listdir(a["data"])))
    out["list_home"] = attempt(lambda: len(os.listdir(a["home"])))
    out["list_home_ssh"] = attempt(lambda: len(os.listdir(os.path.join(a["home"], ".ssh"))))
    out["read_home_zshrc"] = attempt(lambda: len(open(os.path.join(a["home"], ".zshrc")).read()))
    out["write_workspace"] = attempt(lambda: open(os.path.join(a["workspace"], "w.txt"), "w").write("x"))
    out["write_outside"] = attempt(write_outside)
    out["write_tmp"] = attempt(lambda: open("/tmp/iris-probe-" + a["marker"], "w").write("x"))
    out["connect_egress_port"] = attempt(lambda: connect(a["good"]))
    out["connect_other_port"] = attempt(lambda: connect(a["bad"]))
    out["connect_public"] = attempt(public)
    out["udp_public"] = attempt(udp)
    out["connect_unix_socket"] = attempt(unix)
    out["signal_parent"] = attempt(lambda: os.kill(a["parent"], 0) or "signaled")
    out["exec_echo"] = attempt(lambda: run(["/bin/echo", "hi"]))
    out["launchctl_submit"] = attempt(lambda: run(["/bin/launchctl", "submit", "-l", "com.iris.probe.escape." + a["marker"], "--", "/usr/bin/true"]))
    out["pbpaste"] = attempt(lambda: run(["/usr/bin/pbpaste"]))
    out["ps_all"] = attempt(lambda: run(["/bin/ps", "-axo", "pid"]))
    with open(os.path.join(a["workspace"], "result.json"), "w") as f:
        json.dump(out, f, indent=1)


# --- main ------------------------------------------------------------------


def main():
    if sys.platform != "darwin":
        sys.exit("macOS only")
    marker = "irisprobe%d" % os.getpid()
    report = {
        "macos": subprocess.run(["/usr/bin/sw_vers", "-productVersion"], capture_output=True, text=True).stdout.strip(),
        "arch": os.uname().machine,
        "python": sys.version.split()[0],
        "probe_coalition": coalition(os.getpid()),
    }
    # A profile for the escapers in the lifetime tests: the containment
    # profile without a workspace, so a sandboxed escaper writes its beat.
    sb_dir = os.path.realpath(tempfile.mkdtemp(prefix="iris-probe-sb-"))
    tmp_root = os.path.realpath(tempfile.gettempdir())
    lifetime_profile = os.path.join(sb_dir, "lifetime.sb")
    read_extra = sorted({os.path.realpath(sys.base_prefix), os.path.dirname(os.path.realpath(sys.executable)), sb_dir, tmp_root}
                        | ({"/opt/homebrew"} if os.path.isdir("/opt/homebrew") else set()))
    write_profile(lifetime_profile, tmp_root, sb_dir, 9, read_extra)
    try:
        report["containment"] = test_containment(marker)
        report["kill_loop"] = [test_kill_loop(marker, None, 200 + i) for i in range(3)]
        signal.signal(signal.SIGURG, lambda *_: URG.append(1))
        report["init_killer"] = []
        for i in range(10):
            r = test_init_killer(marker, lifetime_profile, 400 + i)
            report["init_killer"].append(r)
            if r.get("aborted") or r.get("error"):
                break
    finally:
        left = marker_pids(marker)
        report["cleanup_marker_pids_found"] = len(left)
        for pid in left:
            try:
                os.kill(pid, signal.SIGKILL)
            except OSError:
                pass
    text = json.dumps(report, indent=1, default=str)
    out = os.path.join(tempfile.gettempdir(), "iris-probe-report.json")
    with open(out, "w") as f:
        f.write(text)
    print(text)
    print("report: " + out, file=sys.stderr)


if __name__ == "__main__":
    if len(sys.argv) > 1 and sys.argv[1] in ("hopper", "sleeper", "helper"):
        mode, outdir, deadline, marker = sys.argv[1], sys.argv[2], float(sys.argv[3]), sys.argv[4]
        if mode == "hopper":
            run_hopper(outdir, deadline, marker)
        elif mode == "sleeper":
            run_sleeper(outdir, deadline, marker)
        else:
            run_helper(outdir, deadline, marker, sys.argv[5] if len(sys.argv) > 5 else "")
    elif len(sys.argv) > 1 and sys.argv[1] == "init":
        run_init(sys.argv[2], float(sys.argv[3]), sys.argv[4])
    elif len(sys.argv) > 2 and sys.argv[1] == "inside":
        run_inside(sys.argv[2])
    else:
        main()
