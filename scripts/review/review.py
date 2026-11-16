#!/usr/bin/env python3
"""Codex review of one pull request, and an optional merge of the exact reviewed head.

    python3 scripts/review/review.py --pr <number> [--merge] [--dry-run] [--force]
    python3 scripts/review/review.py --pre

`--pre` is the pre-review: the same review on HEAD of the branch before the PR is open.
It posts nothing. It writes .local-ci/pre-review.json, which local CI needs for a large
branch.

The steps:
 1. Read the PR through the GitHub REST API (cloud sessions block GraphQL).
 2. Stop if the author is not trusted, the PR is not open, or it does not target the base branch.
 3. Stop if the head is not a valid local CI proof commit.
 4. Make a fresh detached worktree of the exact head.
 5. Run Codex there: ephemeral, no user config, no rules, read-only sandbox, a clean
    shell environment, and a JSON schema for the result.
 5a. A change to a spec file (review.spec_paths) adds the spec rules of spec.md.
 6. Decide the verdict in Python: any P0, P1 or P2 finding blocks. P3 is advice.
    After the first round, a P2 finding on code that did not change since the last
    reviewed head does not block. It goes to a follow-up card.
 7. Read the PR again. If the head, the base branch or the body changed, post nothing.
 8. Post one comment with the verdict and the findings.
 9. With --merge and a clean verdict: mark the PR ready and merge the reviewed head SHA.

Exit codes: 0 clean (and merged with --merge), 2 blocked or stopped with a reason, 1 error.
Standard library only. Needs `gh` (signed in) and `codex` (signed in) on PATH.
"""

from __future__ import annotations

import argparse
import fnmatch
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional, Sequence

HERE = Path(__file__).resolve().parent
MARKER = "<!-- starter-review v1"
BLOCKING = ("P0", "P1", "P2")
SEVERITIES = ("P0", "P1", "P2", "P3")
WRITERS = ("OWNER", "MEMBER", "COLLABORATOR")
DEFAULTS = {"model": "gpt-6-sol", "effort": "medium", "merge_method": "squash",
            "trusted_authors": [], "codex_timeout_minutes": 30, "check_wait_seconds": 300,
            "max_rounds": 5, "spec_paths": ["docs/spec/**", "docs/specs/**"],
            # The first round and the pre-review use first_effort; later rounds use effort.
            "first_effort": "high"}
FIX_MARKER = "<!-- starter-review-fix-findings "
PRE_BODY = ("(This is a pre-review. The PR is not open yet and has no description. Do not report a "
            "finding about the description or the release notes.)")
# The reviewer's shell gets this PATH and nothing else from the host. Without a PATH,
# the Linux sandbox cannot find bwrap and every command of the reviewer fails.
SANDBOX_PATH = 'shell_environment_policy.set={PATH="/usr/local/bin:/usr/bin:/bin"}'


class Stop(Exception):
    """The review ends with a reason and exit code 2."""


# ------------------------------------------------------------------ GitHub REST

Api = Callable[..., Any]


def gh_api(method: str, path: str, payload: Optional[Dict[str, Any]] = None) -> Any:
    command = ["gh", "api", "-X", method, path]
    if payload is not None:
        command += ["--input", "-"]
    done = subprocess.run(command, input=json.dumps(payload) if payload is not None else None,
                          capture_output=True, text=True, timeout=90)
    if done.returncode != 0:
        raise RuntimeError(f"gh api -X {method} {path} failed: {(done.stderr or done.stdout).strip()}")
    return json.loads(done.stdout) if done.stdout.strip() else {}


def get_all(api: Api, path: str) -> List[Dict[str, Any]]:
    rows: List[Dict[str, Any]] = []
    page = 1
    while True:
        batch = api("GET", f"{path}{'&' if '?' in path else '?'}per_page=100&page={page}")
        rows.extend(batch)
        if len(batch) < 100:
            return rows
        page += 1


def repo_slug(root: Path) -> str:
    """owner/name from the origin URL. Cloud sessions use a proxy URL that ends the same way."""
    if os.environ.get("STARTER_REPO"):
        return os.environ["STARTER_REPO"]
    url = git(root, "remote", "get-url", "origin")
    match = re.search(r"[:/]([^/:]+)/([^/]+?)(?:\.git)?/?$", url)
    if not match:
        raise Stop(f"cannot read owner/name from the origin URL {url!r}; set STARTER_REPO=owner/name")
    return f"{match.group(1)}/{match.group(2)}"


# ------------------------------------------------------------------ git

def git(root: Path, *args: str, check: bool = True) -> str:
    done = subprocess.run(["git", *args], cwd=root, capture_output=True, text=True)
    if check and done.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)} failed: {(done.stderr or done.stdout).strip()}")
    return done.stdout.strip()


def load_config(root: Path) -> Dict[str, Any]:
    data = json.loads((root / "starter.json").read_text(encoding="utf-8"))
    review = {**DEFAULTS, **data.get("review", {})}
    review["base"] = data.get("base", "develop")
    return review


# ------------------------------------------------------------------ the review

def body_hash(body: Optional[str]) -> str:
    return hashlib.sha256((body or "").replace("\r\n", "\n").encode()).hexdigest()[:12]


def marker(head: str, merge_base: str, body: str, verdict: str) -> str:
    return f"{MARKER} head={head} base={merge_base} body={body} verdict={verdict} -->"


def find_previous(comments: Sequence[Dict[str, Any]], head: str, merge_base: str, body: str) -> Optional[str]:
    """The verdict of an earlier review comment for the same inputs, if one exists."""
    want = re.compile(re.escape(f"{MARKER} head={head} base={merge_base} body={body} verdict=") + r"(\w+) -->")
    for comment in reversed(list(comments)):
        found = want.search(comment.get("body") or "")
        if found:
            return found.group(1)
    return None


def review_comments(comments: Sequence[Dict[str, Any]]) -> List[Dict[str, Any]]:
    """The comments that this command posted: the marker comes first, and GitHub marks the
    author as an owner, member or collaborator of the repository (no extra API call, which
    the cloud proxy blocks)."""
    return [c for c in comments if (c.get("body") or "").startswith(MARKER)
            and c.get("author_association") in WRITERS]


def blocked_rounds(comments: Sequence[Dict[str, Any]]) -> int:
    return sum(1 for c in comments if re.search(re.escape(MARKER) + r" .*verdict=blocked -->", c.get("body") or ""))


def last_review(comments: Sequence[Dict[str, Any]], head: str) -> Optional[Dict[str, Any]]:
    """The newest review comment of another head. A second review of the same head (--force)
    must not see its own head as the last reviewed head: no line changed since then."""
    for comment in reversed(list(comments)):
        found = re.search(re.escape(MARKER) + r" head=(\w+) ", comment.get("body") or "")
        if found and found.group(1) != head:
            return {"head": found.group(1), "body": comment["body"]}
    return None


def changed_since(root: Path, last: Optional[str], head: str) -> Callable[[Dict[str, Any]], bool]:
    """A test for one finding: is it on code that changed since the last reviewed head?
    With no last head, or a last head that is not an ancestor (a rebase), all code is new."""
    if not last or subprocess.run(["git", "merge-base", "--is-ancestor", last, head], cwd=root,
                                  capture_output=True).returncode != 0:
        return lambda finding: True

    def run(*args: str) -> subprocess.CompletedProcess:
        return subprocess.run(["git", *args], cwd=root, capture_output=True, text=True)

    def changed(finding: Dict[str, Any]) -> bool:
        """Git answers the question. Each doubt (not a file, no cited line, no such line, a
        change with no lines) counts as changed, so the finding stays blocking."""
        path, line = finding["path"], finding.get("line") or 0
        if run("cat-file", "-e", f"{head}:{path}").returncode != 0:
            return True  # not a file of the head, for example the PR description
        counts = run("diff", "--numstat", last, head, "--", path).stdout.split()[:2]
        if not counts:
            return False  # the file did not change
        if not line or counts in (["0", "0"], ["-", "-"]):
            return True  # no cited line, or a change with no lines (a mode change, a binary file)
        blame = run("blame", "--porcelain", "-L", f"{line},{line}", head, "--", path)
        if blame.returncode != 0 or not blame.stdout:
            return True
        # shortcut: a fix that only deletes lines changes no line of the head, so a finding on
        # the next line counts as older code. Upgrade if such findings escape.
        return run("merge-base", "--is-ancestor", blame.stdout.split()[0], last).returncode != 0
    return changed


def spec_files(files: Sequence[str], patterns: Sequence[str]) -> List[str]:
    return [f for f in files if any(fnmatch.fnmatchcase(f, p) for p in patterns)]


def build_prompt(root: Path, pr: Dict[str, Any], head: str, merge_base: str, base_ref: str,
                 specs: Sequence[str] = ()) -> str:
    template = (HERE / "prompt.md").read_text(encoding="utf-8")
    rules_file = HERE / "rules.md"
    rules = rules_file.read_text(encoding="utf-8") if rules_file.exists() else "(none)"
    facts = "\n".join([
        f"- Pull request: #{pr['number']} {pr['title']}",
        f"- Head commit (the reviewed code): {head}",
        f"- Merge base with {base_ref}: {merge_base}",
        f"- The change: `git diff {merge_base} {head}`",
        f"- Changed files: `git diff --stat {merge_base} {head}`",
    ])
    body = (pr.get("body") or "(empty)").strip()
    prompt = (template.replace("{{FACTS}}", facts).replace("{{RULES}}", rules.strip())
              .replace("{{BODY}}", body))
    if specs:
        spec_rules = (HERE / "spec.md").read_text(encoding="utf-8").strip()
        prompt += "\n\n" + spec_rules.replace("{{SPECS}}", "\n".join(f"- `{f}`" for f in specs)) + "\n"
    return prompt


def codex_command(codex: str, worktree: Path, schema: Path, model: str, effort: str) -> List[str]:
    return [codex, "exec", "--json", "--color", "never", "--ephemeral", "--ignore-user-config", "--ignore-rules",
            "-m", model, "-c", f'model_reasoning_effort="{effort}"', "-c", 'approval_policy="never"',
            "-c", "shell_environment_policy.inherit=none", "-c", SANDBOX_PATH,
            "-s", "read-only", "-C", str(worktree), "--output-schema", str(schema), "-"]


def final_message(stdout: str) -> Optional[str]:
    """The text of the last agent message in the `codex exec --json` event stream."""
    message = None
    for line in stdout.splitlines():
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        item = event.get("item") if isinstance(event, dict) else None
        if event.get("type") == "item.completed" and isinstance(item, dict) and item.get("type") == "agent_message":
            message = item.get("text")
    return message


def check_result(result: Any, number: int, head: str) -> Dict[str, Any]:
    """Validate the Codex result. Raises Stop if it does not follow the schema."""
    if not isinstance(result, dict):
        raise Stop("Codex returned no JSON object")
    if result.get("verdict") not in ("clean", "blocked"):
        raise Stop(f"Codex returned an unknown verdict {result.get('verdict')!r}")
    if result.get("pr_number") != number or not head.startswith(str(result.get("head_sha", "-"))):
        raise Stop("Codex reviewed another PR or another head")
    findings = result.get("findings")
    if not isinstance(findings, list):
        raise Stop("Codex returned no findings list")
    for finding in findings:
        if not isinstance(finding, dict) or finding.get("severity") not in SEVERITIES:
            raise Stop(f"Codex returned a finding with no valid severity: {finding!r}")
        for key in ("path", "title", "rationale", "requested_fix"):
            if not isinstance(finding.get(key), str):
                raise Stop(f"Codex returned a finding with no {key}")
    return result


def decide(result: Dict[str, Any], changed: Callable[[Dict[str, Any]], bool] = lambda finding: True) -> str:
    """Marks each P2 finding on unchanged code as `older` (it does not block), then decides."""
    for finding in result["findings"]:
        finding["older"] = finding["severity"] == "P2" and not changed(finding)
    serious = [f for f in result["findings"] if f["severity"] in BLOCKING]
    if any(not f["older"] for f in serious):
        return "blocked"
    # A blocked verdict with no finding to explain it stays blocked.
    return "blocked" if result["verdict"] == "blocked" and not serious else "clean"


def run_codex(root: Path, config: Dict[str, Any], pr: Dict[str, Any], head: str, merge_base: str,
              runner: Callable[..., subprocess.CompletedProcess] = subprocess.run,
              specs: Sequence[str] = ()) -> Dict[str, Any]:
    codex = shutil.which("codex")
    if not codex:
        raise Stop("codex is not installed; install it and sign in (codex login), then run the review again")
    work = Path(tempfile.mkdtemp(prefix="starter-review-"))
    worktree = work / "head"
    git(root, "worktree", "add", "-q", "--detach", str(worktree), head)
    try:
        prompt = build_prompt(root, pr, head, merge_base, config["base"], specs)
        command = codex_command(codex, worktree, HERE / "schema.json", config["model"], config["effort"])
        print(f"review: Codex reviews {head[:12]} ({config['model']}, {config['effort']})", flush=True)
        done = runner(command, input=prompt, capture_output=True, text=True,
                      timeout=int(config["codex_timeout_minutes"]) * 60)
        (root / ".local-ci").mkdir(exist_ok=True)
        (root / ".local-ci" / "review-codex.jsonl").write_text(done.stdout or "", encoding="utf-8")
        if done.returncode != 0:
            tail = "\n".join((done.stderr or done.stdout or "").splitlines()[-20:])
            raise Stop(f"Codex failed (exit {done.returncode}). Run the review again. Last lines:\n{tail}")
        text = final_message(done.stdout or "")
        try:
            result = json.loads(text or "")
        except json.JSONDecodeError as error:
            raise Stop(f"Codex returned text that is not JSON: {error}") from error
        return check_result(result, pr["number"], head)
    finally:
        git(root, "worktree", "remove", "--force", str(worktree), check=False)
        shutil.rmtree(work, ignore_errors=True)


def render_comment(result: Dict[str, Any], verdict: str, mark: str, repair: bool = False) -> str:
    lines = [mark, f"## Codex review: {verdict}", "", result.get("summary", "").strip(), ""]
    findings = sorted(result["findings"], key=lambda f: f["severity"])
    if findings:
        lines += ["| Severity | Where | Finding | Requested fix |", "| --- | --- | --- | --- |"]
        for f in findings:
            where = f"`{f['path']}:{f.get('line', 0)}`"
            severity = f["severity"] + (" (older code)" if f.get("older") else "")
            cells = [severity, where, f"**{f['title']}** {f['rationale']}", f["requested_fix"]]
            lines.append("| " + " | ".join(c.replace("|", "\\|").replace("\n", " ") for c in cells) + " |")
        lines.append("")
    risks = result.get("residual_risks") or []
    if risks:
        lines += ["Residual risks:", *[f"- {r}" for r in risks], ""]
    lines.append("P0, P1 and P2 findings block the merge. P3 findings are advice.")
    if any(f.get("older") for f in findings):
        lines.append("A P2 finding on older code (code that did not change since the last review) does "
                     "not block. Put these findings on one follow-up card.")
    if repair:
        lines.append("Repair signal: this round and the round before found a defect in the fix code. "
                     "Do not patch one more case. Name the invariant and redesign the part "
                     "(AGENTS.md, \"Fix the class\").")
    return "\n".join(lines)


def wait_for_checks(api: Api, slug: str, head: str, seconds: int) -> None:
    """Wait for the check runs of the head to finish. Stop if one failed."""
    deadline = time.monotonic() + seconds
    while True:
        runs = api("GET", f"repos/{slug}/commits/{head}/check-runs?per_page=100").get("check_runs", [])
        pending = [r["name"] for r in runs if r.get("status") != "completed"]
        failed = [r["name"] for r in runs if r.get("status") == "completed"
                  and r.get("conclusion") not in ("success", "skipped", "neutral")]
        if failed:
            raise Stop(f"checks failed on the head: {', '.join(failed)}")
        if not pending:
            return
        if time.monotonic() > deadline:
            raise Stop(f"checks did not finish in {seconds}s: {', '.join(pending)}; run the review again")
        time.sleep(10)


def mark_ready(api: Api, slug: str, number: int) -> None:
    # Cloud sessions: the proxy gives a REST route for this. Elsewhere: gh pr ready (GraphQL).
    try:
        api("POST", f"repos/{slug}/pulls/{number}/ccr/ready_for_review")
        return
    except RuntimeError:
        pass
    done = subprocess.run(["gh", "pr", "ready", str(number), "--repo", slug], capture_output=True, text=True)
    if done.returncode != 0:
        raise Stop(f"cannot mark PR #{number} ready: {(done.stderr or done.stdout).strip()}")


def review(number: int, merge: bool, dry_run: bool, force: bool, api: Api = gh_api,
           runner: Callable[..., subprocess.CompletedProcess] = subprocess.run) -> int:
    root = Path(git(Path.cwd(), "rev-parse", "--show-toplevel"))
    config = load_config(root)
    slug = repo_slug(root)
    pr = api("GET", f"repos/{slug}/pulls/{number}")
    head = pr["head"]["sha"]
    author = pr["user"]["login"]
    trusted = config["trusted_authors"] or [api("GET", "user")["login"]]
    if author not in trusted:
        raise Stop(f"the author {author} is not in review.trusted_authors ({', '.join(trusted)})")
    if pr["state"] != "open":
        raise Stop(f"PR #{number} is {pr['state']}")
    if pr["base"]["ref"] != config["base"]:
        raise Stop(f"PR #{number} targets {pr['base']['ref']}, not {config['base']}")
    if pr["head"]["repo"] is None or pr["head"]["repo"]["full_name"] != slug:
        raise Stop("the head branch is in another repository; review only branches of this repository")
    branch = pr["head"]["ref"]
    git(root, "fetch", "-q", "origin", config["base"], f"+refs/heads/{branch}:refs/remotes/origin/{branch}")
    if git(root, "rev-parse", f"refs/remotes/origin/{branch}") != head:
        raise Stop("the branch moved while the review started; run the review again")
    proof = subprocess.run([sys.executable, str(root / "scripts/local-ci/local_ci.py"), "--verify", head],
                           cwd=root, capture_output=True, text=True)
    if proof.returncode != 0:
        raise Stop(f"no local CI proof on the head: {proof.stdout.strip() or proof.stderr.strip()}")
    merge_base = git(root, "merge-base", f"origin/{config['base']}", head)
    body = body_hash(pr.get("body"))
    comments = review_comments(get_all(api, f"repos/{slug}/issues/{number}/comments"))
    previous = None if force else find_previous(comments, head, merge_base, body)
    if previous:
        print(f"review: an earlier review of the same head, base and body says {previous}; "
              "no new Codex run (use --force to review again)", flush=True)
        verdict = previous
    else:
        rounds = blocked_rounds(comments)
        if rounds >= int(config["max_rounds"]) and not force:
            raise Stop(f"PR #{number} has {rounds} blocked review rounds (review.max_rounds is "
                       f"{config['max_rounds']}). Stop and tell the user which subsystem the findings hit. "
                       "Run with --force only after the user says to continue.")
        changed = git(root, "diff", "--name-only", merge_base, head).splitlines()
        specs = spec_files(changed, config["spec_paths"])
        last = last_review(comments, head)
        if not comments:
            config = {**config, "effort": config["first_effort"]}
        result = run_codex(root, config, pr, head, merge_base, runner, specs)
        verdict = decide(result, changed_since(root, last and last["head"], head))
        # Defects in the code that changed since the last review: the fix of the round before.
        fixes = sum(1 for f in result["findings"] if last and f["severity"] in BLOCKING and not f["older"])
        repair = bool(fixes and last and re.search(re.escape(FIX_MARKER) + r"[1-9]", last["body"]))
        text = render_comment(result, verdict, marker(head, merge_base, body, verdict), repair)
        text += f"\n{FIX_MARKER}{fixes} -->"
        print(text, flush=True)
        live = api("GET", f"repos/{slug}/pulls/{number}")
        if live["head"]["sha"] != head or body_hash(live.get("body")) != body or live["base"]["ref"] != config["base"]:
            raise Stop("the PR changed during the review (head, body or base); nothing posted; run the review again")
        if dry_run:
            print("review: --dry-run, nothing posted", flush=True)
        else:
            api("POST", f"repos/{slug}/issues/{number}/comments", {"body": text})
            print(f"review: posted the {verdict} review on PR #{number}", flush=True)
    if verdict != "clean":
        print("review: blocked. Fix each P0, P1 and P2 finding, commit, run scripts/local-ci/run.sh, "
              "then run the review again.", flush=True)
        return 2
    if not merge or dry_run:
        return 0
    if pr.get("draft"):
        mark_ready(api, slug, number)
    wait_for_checks(api, slug, head, int(config["check_wait_seconds"]))
    try:
        api("PUT", f"repos/{slug}/pulls/{number}/merge", {"sha": head, "merge_method": config["merge_method"]})
    except RuntimeError as error:
        raise Stop(f"GitHub refused the merge: {error}") from error
    if not api("GET", f"repos/{slug}/pulls/{number}").get("merged"):
        raise Stop("GitHub accepted the merge call, but the PR does not show as merged; check the PR")
    print(f"review: merged PR #{number} at {head[:12]} into {config['base']}", flush=True)
    return 0


def pre_review(runner: Callable[..., subprocess.CompletedProcess] = subprocess.run) -> int:
    """The review of HEAD before the PR is open. Posts nothing. Writes the record for local CI."""
    root = Path(git(Path.cwd(), "rev-parse", "--show-toplevel"))
    config = load_config(root)
    config["effort"] = config["first_effort"]
    if git(root, "status", "--porcelain"):
        raise Stop("the working tree has changes; commit them first, the pre-review reads HEAD")
    head = git(root, "rev-parse", "HEAD")
    git(root, "fetch", "-q", "origin", config["base"])
    merge_base = git(root, "merge-base", f"origin/{config['base']}", head)
    changed = git(root, "diff", "--name-only", merge_base, head).splitlines()
    # The schema needs a PR number of 1 or more. The PR does not exist yet.
    pr = {"number": 1, "title": f"pre-review of the branch {git(root, 'rev-parse', '--abbrev-ref', 'HEAD')}",
          "body": PRE_BODY}
    result = run_codex(root, config, pr, head, merge_base, runner, spec_files(changed, config["spec_paths"]))
    verdict = decide(result)
    print(render_comment(result, verdict, f"pre-review of {head}"), flush=True)
    (root / ".local-ci" / "pre-review.json").write_text(
        json.dumps({"head": head, "verdict": verdict, "findings": len(result["findings"])}), encoding="utf-8")
    print("review: pre-review recorded. Fix each P0, P1 and P2 finding, then run scripts/local-ci/run.sh.",
          flush=True)
    return 0 if verdict == "clean" else 2


def main(argv: Optional[Sequence[str]] = None) -> int:
    parser = argparse.ArgumentParser(description="Codex review of one PR, then an optional merge.")
    target = parser.add_mutually_exclusive_group(required=True)
    target.add_argument("--pr", type=int, help="the PR number")
    target.add_argument("--pre", action="store_true",
                        help="pre-review: review HEAD before the PR is open; post nothing")
    parser.add_argument("--merge", action="store_true", help="merge the reviewed head if the review is clean")
    parser.add_argument("--dry-run", action="store_true", help="print the review; post and merge nothing")
    parser.add_argument("--force", action="store_true",
                        help="review again when a review of the same inputs exists, or past review.max_rounds")
    args = parser.parse_args(argv)
    try:
        return pre_review() if args.pre else review(args.pr, args.merge, args.dry_run, args.force)
    except Stop as reason:
        print(f"review: stopped: {reason}", file=sys.stderr)
        return 2
    except (RuntimeError, subprocess.TimeoutExpired, KeyError) as error:
        print(f"review: error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
