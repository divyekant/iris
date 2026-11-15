#!/usr/bin/env python3
"""Local CI for pull requests: test the areas that a branch changes, then push a proof commit.

The areas and their commands are in starter.json at the repository root. This file is
the same in every product that uses the starter kit. Change the commands in
starter.json, not here. Standard library only, so any python3 (3.9 or later) can run it.

Modes:
  (none)        run the changed areas, write the proof commit, push it
  --no-proof    run the changed areas, write nothing, push nothing
  --all         run every area, not only the changed areas
  --prepare     run the setup of every area and no test (the session warm-up)
  --verify REV  exit 0 if REV is a valid proof commit, else print the reason and exit 1
  --pre-push    the fast checks of the git pre-push hook (reads the hook's stdin)
  --ci          run every area on any checkout, write no proof (the release CI on GitHub Actions)
"""

from __future__ import annotations

import argparse
import fcntl
import hashlib
import json
import os
import re
import subprocess
import sys
import threading
import time
import urllib.parse
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from typing import Any, Dict, List, Optional, Sequence, Tuple

CONFIG = "starter.json"
PROOF_PREFIX = "local-ci: v1 "
TREE_TRAILER = "Local-CI-Tree: "
# One line per area in the proof body: `<area> <fingerprint> ran` or
# `<area> <fingerprint> reused <sha of the proof that ran it>`.
AREA_TRAILER = "Local-CI-Area: "
# A change to one of these files selects every area.
DEFAULT_ALWAYS = (CONFIG, "scripts/local-ci/**")
PROTECTED = ("main", "master", "develop")
AREA_KEYS = {"paths", "setup", "setup_inputs", "check", "test", "tools", "env"}
ENV_NAME = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
LOG_TAIL = 80
FIXED_ENV = {"LANG": "C.UTF-8", "LC_ALL": "C.UTF-8", "TZ": "UTC", "CI": "1", "LOCAL_CI": "1"}
# The tests run code from the branch. They get only these host variables, so no token
# of the cloud setup (GH_TOKEN, API keys, cloud keys) reaches them.
KEEP_ENV = ("PATH", "TERM", "TMPDIR", "USER", "LOGNAME", "SHELL")
# A Claude cloud session sends all traffic through a proxy. Package managers need its
# address and its CA bundle. These hold paths and addresses, not secrets.
CA_ENV = ("SSL_CERT_FILE", "SSL_CERT_DIR", "REQUESTS_CA_BUNDLE", "CURL_CA_BUNDLE", "PIP_CERT",
          "NODE_EXTRA_CA_CERTS", "UV_NATIVE_TLS", "npm_config_cafile", "NPM_CONFIG_CAFILE")
PROXY_ENV = ("HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy", "ALL_PROXY", "all_proxy")
NO_PROXY_ENV = ("NO_PROXY", "no_proxy")


class Refusal(Exception):
    """A refusal or a failure. The run ends and writes no proof."""


# ------------------------------------------------------------------ git helpers

def git(root: Path, *args: str, check: bool = True) -> str:
    done = subprocess.run(["git", *args], cwd=root, capture_output=True, text=True)
    if check and done.returncode != 0:
        raise Refusal(f"git {' '.join(args)} failed: {(done.stderr or done.stdout).strip()}")
    return done.stdout.strip()


def repo_root() -> Path:
    done = subprocess.run(["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True)
    if done.returncode != 0:
        raise Refusal("run this command inside a git checkout")
    return Path(done.stdout.strip())


# ------------------------------------------------------------------ configuration

def load_config(root: Path) -> Dict[str, Any]:
    path = root / CONFIG
    if not path.is_file():
        raise Refusal(f"{CONFIG} is missing at the repository root")
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        raise Refusal(f"{CONFIG} is not valid JSON: {error}") from error
    if data.get("version") != 1:
        raise Refusal(f"{CONFIG}: version must be 1")
    areas = data.get("areas")
    if not isinstance(areas, dict):
        raise Refusal(f"{CONFIG}: areas must be an object")
    for name, area in areas.items():
        if not re.fullmatch(r"[a-z][a-z0-9-]*", name):
            raise Refusal(f"{CONFIG}: area name {name!r} must be lower case letters, digits and -")
        if not isinstance(area, dict) or not set(area) <= AREA_KEYS:
            raise Refusal(f"{CONFIG}: area {name} may have only {', '.join(sorted(AREA_KEYS))}")
        if not area.get("paths") or not all(isinstance(p, str) for p in area["paths"]):
            raise Refusal(f"{CONFIG}: area {name} needs a list of paths")
        for key in ("setup", "check", "test"):
            if key in area and not isinstance(area[key], str):
                raise Refusal(f"{CONFIG}: {name}.{key} must be one shell command (a string)")
        extra = area.get("env", {})
        if not isinstance(extra, dict) or not all(
                isinstance(k, str) and ENV_NAME.fullmatch(k) and isinstance(v, str) for k, v in extra.items()):
            raise Refusal(f"{CONFIG}: {name}.env must map variable names to strings")
    data.setdefault("base", "develop")
    data.setdefault("always", list(DEFAULT_ALWAYS))
    return data


def glob_regex(pattern: str) -> "re.Pattern[str]":
    """`**/` = any folders, `**` = anything, `*` = anything except /, `?` = one character."""
    out, i = "", 0
    while i < len(pattern):
        if pattern.startswith("**/", i):
            out, i = out + "(?:.*/)?", i + 3
        elif pattern.startswith("**", i):
            out, i = out + ".*", i + 2
        elif pattern[i] == "*":
            out, i = out + "[^/]*", i + 1
        elif pattern[i] == "?":
            out, i = out + "[^/]", i + 1
        else:
            out, i = out + re.escape(pattern[i]), i + 1
    return re.compile(out + r"\Z")


def matches(patterns: Sequence[str], path: str) -> bool:
    return any(glob_regex(p).match(path) for p in patterns)


def select_areas(config: Dict[str, Any], files: Sequence[str]) -> List[str]:
    areas = config["areas"]
    if any(matches(config["always"], f) for f in files):
        return list(areas)
    return [name for name, area in areas.items() if any(matches(area["paths"], f) for f in files)]


# ------------------------------------------------------------------ environment

def network_env() -> Dict[str, str]:
    env = {k: os.environ[k] for k in CA_ENV + NO_PROXY_ENV if k in os.environ}
    for key in PROXY_ENV:
        value = os.environ.get(key)
        if not value:
            continue
        try:
            parts = urllib.parse.urlsplit(value if "://" in value else f"http://{value}")
            parts.port  # a bad port raises ValueError
            withheld = bool(parts.username or parts.password)
        except ValueError:
            withheld = True
        if withheld:
            print(f"local-ci: {key} holds credentials; the commands run without it", flush=True)
        else:
            env[key] = value
    return env


def area_env(root: Path, extra: Dict[str, str]) -> Dict[str, str]:
    """The `env` of an area. `~/x` is a path in the host HOME (for a tool that lives
    there, for example the Rust toolchains in ~/.rustup). `./x` is a path in the checkout."""
    host_home = os.path.expanduser("~")
    out = {}
    for key, value in extra.items():
        if value.startswith("~/"):
            value = os.path.join(host_home, value[2:])
        elif value.startswith("./"):
            value = str(root / value[2:])
        out[key] = value
    return out


def isolated_env(root: Path, area: str, workers: int,
                 extra: Optional[Dict[str, str]] = None) -> Dict[str, str]:
    """No host credentials and a private HOME under .local-ci/. This is not a sandbox:
    a command can still read a file by its absolute path. `extra` is the `env` of the area."""
    home = root / ".local-ci" / "home"
    home.mkdir(parents=True, exist_ok=True)
    env = {k: os.environ[k] for k in KEEP_ENV if k in os.environ}
    return {**env, **network_env(), **FIXED_ENV, "HOME": str(home),
            **area_env(root, extra or {}),
            "LOCAL_CI_AREA": area, "LOCAL_CI_WORKERS": str(workers)}


def state_dir(root: Path) -> Path:
    path = root / ".local-ci"
    (path / "logs").mkdir(parents=True, exist_ok=True)
    (path / "stamps").mkdir(parents=True, exist_ok=True)
    # The folder ignores itself, so a product needs no .gitignore line for it.
    ignore = path / ".gitignore"
    if not ignore.exists():
        ignore.write_text("*\n", encoding="utf-8")
    return path


# ------------------------------------------------------------------ fingerprints

def blob_lines(root: Path, patterns: Sequence[str], rev: str = "HEAD") -> List[str]:
    rows = git(root, "ls-tree", "-r", "--full-tree", rev).splitlines()
    out = []
    for row in rows:
        meta, _, path = row.partition("\t")
        if matches(patterns, path):
            out.append(f"{meta.split()[2]} {path}")
    return sorted(out)


def tool_versions(root: Path, area: Dict[str, Any], env: Dict[str, str]) -> List[str]:
    lines = []
    for command in area.get("tools", []):
        done = subprocess.run(["sh", "-c", command], cwd=root, env=env, capture_output=True, text=True)
        lines.append(f"{command} => {(done.stdout + done.stderr).strip()}")
    return lines


def fingerprint(root: Path, config: Dict[str, Any], name: str, env: Dict[str, str]) -> str:
    area = config["areas"][name]
    inputs = [*area["paths"], *config["always"], *area.get("setup_inputs", [])]
    text = "\n".join([json.dumps(area, sort_keys=True), *blob_lines(root, inputs),
                      *tool_versions(root, area, env)])
    return hashlib.sha256(text.encode()).hexdigest()[:16]


def setup_stamp(root: Path, area: Dict[str, Any], env: Dict[str, str]) -> str:
    files = []
    for pattern in area.get("setup_inputs", []):
        for path in sorted(root.glob(pattern)):
            if path.is_file():
                files.append(f"{path.relative_to(root)} {hashlib.sha256(path.read_bytes()).hexdigest()}")
    text = "\n".join([area.get("setup", ""), *files, *tool_versions(root, area, env)])
    return hashlib.sha256(text.encode()).hexdigest()


# ------------------------------------------------------------------ proof commits

def head_record(root: Path, rev: str) -> Dict[str, Any]:
    sha = git(root, "rev-parse", "--verify", f"{rev}^{{commit}}")
    parents = git(root, "rev-list", "--parents", "-n", "1", sha).split()[1:]
    return {
        "sha": sha,
        "tree": git(root, "rev-parse", f"{sha}^{{tree}}"),
        "parents": parents,
        "parent_tree": git(root, "rev-parse", f"{sha}~1^{{tree}}") if len(parents) == 1 else "",
        "message": git(root, "log", "-1", "--format=%B", sha),
    }


def proof_problem(head: Dict[str, Any]) -> Optional[str]:
    """None for a valid proof: an empty commit whose recorded tree is its own tree."""
    lines = str(head["message"]).splitlines()
    if not lines or not lines[0].startswith(PROOF_PREFIX):
        return "the head commit is not a local CI proof commit (run scripts/local-ci/run.sh)"
    if len(head["parents"]) != 1:
        return "the proof commit must have exactly one parent"
    if head["tree"] != head["parent_tree"]:
        return "the proof commit changes files; a proof commit must be empty"
    if f"{TREE_TRAILER}{head['tree']}" not in lines:
        return "the proof commit does not record its own tree"
    return None


def recorded_passes(root: Path, base: str) -> Dict[str, str]:
    """Fingerprints that passed on this branch since the merge base, mapped to the proof
    commit that ran them."""
    passes: Dict[str, str] = {}
    shas = git(root, "rev-list", "--grep", "^" + PROOF_PREFIX, f"{base}..HEAD").split()
    for sha in reversed(shas):
        head = head_record(root, sha)
        if proof_problem(head):
            continue
        for line in str(head["message"]).splitlines():
            if not line.startswith(AREA_TRAILER):
                continue
            parts = line[len(AREA_TRAILER):].split()
            if len(parts) == 3 and parts[2] == "ran":
                passes[parts[1]] = sha
            elif len(parts) == 4 and parts[2] == "reused" and re.fullmatch(r"[0-9a-f]{40}", parts[3]):
                passes[parts[1]] = parts[3]
    return passes


def current_branch(root: Path, base: str) -> str:
    done = subprocess.run(["git", "symbolic-ref", "-q", "HEAD"], cwd=root, capture_output=True, text=True)
    ref = done.stdout.strip()
    if done.returncode != 0 or not ref.startswith("refs/heads/"):
        raise Refusal("the checkout is not on a branch; the proof commit must go to a branch")
    name = ref[len("refs/heads/"):]
    if name in (base, *PROTECTED) or name.startswith("release/"):
        raise Refusal(f"`{name}` is not a feature branch; run local CI on a feature branch")
    return name


def push_proof(root: Path, sha: str, branch: str) -> None:
    # Push the proof SHA, not HEAD: a commit made during the run is not tested.
    # --no-verify: the full checks ran, so the fast pre-push hook adds nothing.
    done = subprocess.run(["git", "push", "-q", "--no-verify", "origin", f"{sha}:refs/heads/{branch}"],
                          cwd=root, capture_output=True, text=True)
    remote = git(root, "ls-remote", "origin", f"refs/heads/{branch}", check=False).split()
    if remote and remote[0] == sha:
        return
    detail = (done.stderr or "").strip()
    if "non-fast-forward" in detail or "fetch first" in detail or "[rejected]" in detail:
        raise Refusal("new commits are on the remote branch; pull them, then run local CI again")
    raise Refusal(f"the push of the proof commit failed ({detail}); run local CI again "
                  "to push the same proof without running the tests")


# ------------------------------------------------------------------ the run

class Lock:
    """One run at a time in a checkout. A run waits for a warm-up; it refuses another run."""

    def __init__(self, root: Path, role: str):
        self.path = state_dir(root) / "lock"
        self.handle = self.path.open("a+")
        try:
            fcntl.flock(self.handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            self.handle.seek(0)
            holder = self.handle.read().strip()
            if role == "prepare":
                self.handle.close()
                raise Refusal("a run holds this checkout; no warm-up is necessary")
            if holder != "prepare":
                self.handle.close()
                raise Refusal("another local CI run holds this checkout")
            print("local-ci: the session warm-up is running; this run waits for it", flush=True)
            fcntl.flock(self.handle, fcntl.LOCK_EX)
        self.handle.seek(0)
        self.handle.truncate()
        self.handle.write(role)
        self.handle.flush()

    def close(self) -> None:
        self.handle.close()


def run_step(root: Path, name: str, step: str, command: str, env: Dict[str, str], log: Path) -> None:
    started = time.monotonic()
    with log.open("a", encoding="utf-8") as out:
        out.write(f"\n$ {command}\n")
        out.flush()
        done = subprocess.run(["sh", "-c", command], cwd=root, env=env, stdout=out, stderr=subprocess.STDOUT)
    seconds = time.monotonic() - started
    if done.returncode != 0:
        tail = log.read_text(encoding="utf-8", errors="replace").splitlines()[-LOG_TAIL:]
        print(f"local-ci: {name} {step} failed (exit {done.returncode}, {seconds:.0f}s). "
              f"Last lines of {log.relative_to(root)}:", flush=True)
        print("\n".join(tail), flush=True)
        raise Refusal(f"{name} {step} failed")
    print(f"local-ci: {name} {step} ok ({seconds:.0f}s)", flush=True)


def setup_area(root: Path, name: str, area: Dict[str, Any], env: Dict[str, str], log: Path) -> None:
    if not area.get("setup"):
        return
    stamp_file = state_dir(root) / "stamps" / name
    stamp = setup_stamp(root, area, env)
    if stamp_file.exists() and stamp_file.read_text().strip() == stamp:
        print(f"local-ci: {name} setup skipped (inputs and tools did not change)", flush=True)
        return
    stamp_file.unlink(missing_ok=True)
    run_step(root, name, "setup", area["setup"], env, log)
    stamp_file.write_text(stamp + "\n")


def workers_for(count: int) -> int:
    value = os.environ.get("LOCAL_CI_WORKERS", "")
    if value.isdigit() and int(value) > 0:
        return int(value)
    return max(1, min(count or 1, os.cpu_count() or 1))


def run_areas(root: Path, config: Dict[str, Any], names: Sequence[str], steps: Sequence[str]) -> None:
    """Run the steps of each area. Areas run at the same time, steps of one area in order."""
    if not names:
        return
    workers = workers_for(len(names))
    per_area = max(1, (os.cpu_count() or 1) // workers)
    errors: List[str] = []
    lock = threading.Lock()

    def one(name: str) -> None:
        area = config["areas"][name]
        env = isolated_env(root, name, per_area, area.get("env"))
        log = state_dir(root) / "logs" / f"{name}.log"
        log.write_text(f"local-ci {name} {time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())}\n")
        try:
            for step in steps:
                if step == "setup":
                    setup_area(root, name, area, env, log)
                elif area.get(step):
                    run_step(root, name, step, area[step], env, log)
        except Refusal as error:
            with lock:
                errors.append(str(error))

    with ThreadPoolExecutor(max_workers=workers) as pool:
        list(pool.map(one, names))
    if errors:
        raise Refusal("; ".join(sorted(errors)) + "; no proof written")


def changed_files(root: Path, base_ref: str, rev: str = "HEAD") -> Tuple[str, List[str]]:
    merge_base = git(root, "merge-base", base_ref, rev)
    names = git(root, "diff", "--name-only", "-z", "--no-renames", f"{merge_base}...{rev}")
    return merge_base, [n for n in names.split("\0") if n]


def run_ci(root: Path) -> int:
    """Every area on the checkout as it is: no branch rule, no proof. Used by ci.yml."""
    config = load_config(root)
    names = list(config["areas"])
    print(f"local-ci: CI run of {git(root, 'rev-parse', 'HEAD')[:12]}; areas: {', '.join(names) or 'none'}",
          flush=True)
    started = time.monotonic()
    run_areas(root, config, names, ("setup", "check", "test"))
    print(f"local-ci: all areas passed in {time.monotonic() - started:.0f}s", flush=True)
    return 0


def run(root: Path, prove: bool, every: bool) -> int:
    config = load_config(root)
    base = config["base"]
    branch = current_branch(root, base)
    if git(root, "status", "--porcelain"):
        raise Refusal("the working tree has changes; commit or stash them first")
    lock = Lock(root, "run")
    try:
        head = head_record(root, "HEAD")
        if proof_problem(head) is None:
            print(f"local-ci: {head['sha']} is a valid proof commit; no tests to run", flush=True)
            if prove:
                push_proof(root, head["sha"], branch)
                print(f"local-ci: origin/{branch} is at the proof commit", flush=True)
            return 0
        git(root, "fetch", "-q", "origin", base)
        merge_base, files = changed_files(root, f"origin/{base}")
        names = list(config["areas"]) if every else select_areas(config, files)
        print(f"local-ci: {head['sha'][:12]} changes {len(files)} files; areas: {', '.join(names) or 'none'}",
              flush=True)
        passes = recorded_passes(root, merge_base) if names else {}
        prints = {n: fingerprint(root, config, n, isolated_env(root, n, 1, config["areas"][n].get("env")))
                  for n in names}
        reused = {n: passes[fp] for n, fp in prints.items() if fp in passes}
        for name, sha in reused.items():
            print(f"local-ci: {name} reused: the same files and tools passed in {sha[:12]}", flush=True)
        to_run = [n for n in names if n not in reused]
        started = time.monotonic()
        run_areas(root, config, to_run, ("setup", "check", "test"))
        if git(root, "rev-parse", "HEAD") != head["sha"] or git(root, "status", "--porcelain"):
            raise Refusal("the checkout changed during the run; no proof written")
        summary = f"areas={','.join(names) or 'none'} ran={len(to_run)} reused={len(reused)} os={sys.platform}"
        print(f"local-ci: all steps passed in {time.monotonic() - started:.0f}s | {summary}", flush=True)
        if not prove:
            return 0
        lines = [f"{n} {prints[n]} reused {reused[n]}" if n in reused else f"{n} {prints[n]} ran" for n in names]
        body = "\n".join([TREE_TRAILER + head["tree"], *(AREA_TRAILER + line for line in lines)])
        git(root, "commit", "-q", "--allow-empty", "--no-verify", "-m", PROOF_PREFIX + summary, "-m", body)
        proof = git(root, "rev-parse", "HEAD")
        push_proof(root, proof, branch)
        print(f"local-ci: pushed proof commit {proof} for tree {head['tree']} to origin/{branch}", flush=True)
        return 0
    finally:
        lock.close()


def prepare(root: Path) -> int:
    config = load_config(root)
    lock = Lock(root, "prepare")
    try:
        run_areas(root, config, list(config["areas"]), ("setup",))
        print("local-ci: warm-up done", flush=True)
        return 0
    finally:
        lock.close()


def pre_push(root: Path, lines: Sequence[str]) -> int:
    """Run the `check` command of each area that the pushed commits change."""
    config = load_config(root)
    base_ref = f"origin/{config['base']}"
    files: List[str] = []
    for line in lines:
        parts = line.split()
        if len(parts) != 4 or set(parts[1]) == {"0"}:
            continue  # a malformed line or a deleted branch
        local, remote = parts[1], parts[3]
        known = set(remote) != {"0"} and subprocess.run(
            ["git", "cat-file", "-e", f"{remote}^{{commit}}"], cwd=root, capture_output=True).returncode == 0
        if not known:
            done = subprocess.run(["git", "merge-base", base_ref, local], cwd=root, capture_output=True, text=True)
            if done.returncode != 0:
                continue
            remote = done.stdout.strip()
        files += [n for n in git(root, "diff", "--name-only", "-z", remote, local).split("\0") if n]
    names = [n for n in select_areas(config, sorted(set(files))) if config["areas"][n].get("check")]
    ready = [n for n in names if not config["areas"][n].get("setup") or (state_dir(root) / "stamps" / n).exists()]
    for name in sorted(set(names) - set(ready)):
        print(f"pre-push: {name} check skipped (setup has not run; scripts/local-ci/run.sh runs it)",
              file=sys.stderr)
    try:
        run_areas(root, config, ready, ("check",))
    except Refusal as error:
        print(f"pre-push: {error}. To skip the hook one time: git push --no-verify", file=sys.stderr)
        return 1
    return 0


def main(argv: Optional[Sequence[str]] = None) -> int:
    parser = argparse.ArgumentParser(description="Local CI: test the changed areas, then push a proof commit.")
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--no-proof", action="store_true", help="run the areas, write and push no proof")
    mode.add_argument("--prepare", action="store_true", help="run the setup of every area, no tests")
    mode.add_argument("--verify", metavar="REV", help="check that REV is a valid proof commit")
    mode.add_argument("--pre-push", action="store_true", help="fast checks for the git pre-push hook")
    mode.add_argument("--ci", action="store_true", help="every area, no branch rule, no proof (release CI)")
    parser.add_argument("--all", action="store_true", help="run every area, not only the changed areas")
    args = parser.parse_args(argv)
    try:
        root = repo_root()
        if args.verify:
            problem = proof_problem(head_record(root, args.verify))
            print(problem or "local-ci: valid proof commit")
            return 1 if problem else 0
        if args.prepare:
            return prepare(root)
        if args.ci:
            return run_ci(root)
        if args.pre_push:
            return pre_push(root, sys.stdin.read().splitlines())
        return run(root, prove=not args.no_proof, every=args.all)
    except Refusal as error:
        print(f"local-ci: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
