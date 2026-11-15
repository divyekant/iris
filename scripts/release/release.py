#!/usr/bin/env python3
"""Release cycle: develop -> release/v<YYYY.MM.DD.N> PR -> main, then the tag and the GitHub Release.

    python3 scripts/release/release.py open     # open or move the release PR
    python3 scripts/release/release.py check    # merge a green release PR, then tag and release
    python3 scripts/release/release.py notes    # print the notes of the next release

.github/workflows/release.yml runs `open` and `check`. It runs them with the secret
RELEASE_TOKEN: a merge made with the default GITHUB_TOKEN starts no other workflow, so
deploy.yml would not run on the push to main.

The release notes come from the `## Release Notes` section of each PR that merged into
develop since the last release. Standard library only. Needs `git` and `gh`.
"""

from __future__ import annotations

import argparse
import datetime
import json
import re
import subprocess
import sys
from pathlib import Path
from typing import Any, Callable, Dict, List, Optional, Sequence, Tuple

MARKER = "<!-- starter-release"
CATEGORIES = ("User-facing", "Internal", "Migrations/config/env", "Docs/version/dependency bumps", "Risk/rollback")
EMPTY = {"", "-", "none", "n/a", "na", "no", "nothing"}
CI_CHECK = "ci"


class Stop(Exception):
    """The run ends with a reason and exit code 1 (the workflow run fails, so GitHub tells the owner)."""


def gh_api(method: str, path: str, payload: Optional[Dict[str, Any]] = None) -> Any:
    command = ["gh", "api", "-X", method, path]
    if payload is not None:
        command += ["--input", "-"]
    done = subprocess.run(command, input=json.dumps(payload) if payload is not None else None,
                          capture_output=True, text=True, timeout=90)
    if done.returncode != 0:
        raise RuntimeError(f"gh api -X {method} {path} failed: {(done.stderr or done.stdout).strip()}")
    return json.loads(done.stdout) if done.stdout.strip() else {}


Api = Callable[..., Any]


def git(root: Path, *args: str, check: bool = True) -> str:
    done = subprocess.run(["git", *args], cwd=root, capture_output=True, text=True)
    if check and done.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)} failed: {(done.stderr or done.stdout).strip()}")
    return done.stdout.strip()


def settings(root: Path) -> Tuple[str, str]:
    data = json.loads((root / "starter.json").read_text(encoding="utf-8"))
    return data.get("base", "develop"), data.get("release", {}).get("branch", "main")


def slug_of(root: Path) -> str:
    import os
    if os.environ.get("GITHUB_REPOSITORY"):
        return os.environ["GITHUB_REPOSITORY"]
    url = git(root, "remote", "get-url", "origin")
    match = re.search(r"[:/]([^/:]+)/([^/]+?)(?:\.git)?/?$", url)
    if not match:
        raise Stop(f"cannot read owner/name from {url!r}; set GITHUB_REPOSITORY=owner/name")
    return f"{match.group(1)}/{match.group(2)}"


# ------------------------------------------------------------------ notes

def notes_of(body: Optional[str]) -> Dict[str, List[str]]:
    """The entries of each category in the `## Release Notes` section of one PR body."""
    out: Dict[str, List[str]] = {c: [] for c in CATEGORIES}
    text = (body or "").replace("\r\n", "\n")
    match = re.search(r"^##\s+Release Notes\s*$(.*?)(?=^##\s|\Z)", text, re.M | re.S)
    if not match:
        return out
    current = None
    for line in match.group(1).splitlines():
        if re.match(r"^\s*<!--", line):
            current = None
            continue
        head = re.match(r"^-\s+([^:]+):\s*(.*)$", line)
        if head and head.group(1).strip() in CATEGORIES:
            current = head.group(1).strip()
            rest = head.group(2).strip()
            if rest.lower().rstrip(".") not in EMPTY:
                out[current].append(rest)
            continue
        sub = re.match(r"^\s{2,}[-*]\s+(.*)$", line)
        if current and sub and sub.group(1).strip().lower().rstrip(".") not in EMPTY:
            out[current].append(sub.group(1).strip())
    return out


def merged_prs(api: Api, root: Path, slug: str, base: str, release: str) -> List[Dict[str, Any]]:
    """PRs merged into the base branch whose merge commit is not on the release branch yet."""
    pending = set(git(root, "rev-list", f"origin/{release}..origin/{base}").split())
    rows: List[Dict[str, Any]] = []
    page = 1
    while page <= 10:
        batch = api("GET", f"repos/{slug}/pulls?state=closed&base={base}&sort=updated&direction=desc"
                           f"&per_page=100&page={page}")
        rows += [r for r in batch if r.get("merged_at") and r.get("merge_commit_sha") in pending]
        if len(batch) < 100:
            break
        page += 1
    return sorted(rows, key=lambda r: r["merged_at"])


def render_notes(prs: Sequence[Dict[str, Any]]) -> str:
    sections: Dict[str, List[str]] = {c: [] for c in CATEGORIES}
    for pr in prs:
        for category, entries in notes_of(pr.get("body")).items():
            sections[category] += [f"{entry} (#{pr['number']})" for entry in entries]
    lines = []
    for category in CATEGORIES:
        if sections[category]:
            lines += [f"### {category}", "", *[f"- {e}" for e in sections[category]], ""]
    if not lines:
        lines = ["No release-note impact.", ""]
    lines += ["### Pull requests", "", *[f"- #{p['number']} {p['title']}" for p in prs]]
    return "\n".join(lines).strip() + "\n"


# ------------------------------------------------------------------ tags

def next_tag(root: Path, today: Optional[datetime.date] = None) -> str:
    day = (today or datetime.datetime.now(datetime.timezone.utc).date()).strftime("%Y.%m.%d")
    tags = git(root, "ls-remote", "--tags", "origin", f"refs/tags/v{day}.*").split()
    numbers = [int(m.group(1)) for t in tags if (m := re.fullmatch(rf"refs/tags/v{re.escape(day)}\.(\d+)", t))]
    return f"v{day}.{max(numbers, default=0) + 1}"


def release_pr(api: Api, slug: str, release: str, state: str = "open") -> List[Dict[str, Any]]:
    rows = api("GET", f"repos/{slug}/pulls?state={state}&base={release}&per_page=50")
    return [r for r in rows if r["head"]["ref"].startswith("release/") and MARKER in (r.get("body") or "")]


def body_for(tag: str, notes: str) -> str:
    return f"{MARKER} tag={tag} -->\n## Release {tag}\n\n{notes}"


# ------------------------------------------------------------------ modes

def fetch(root: Path, base: str, release: str) -> None:
    git(root, "fetch", "-q", "origin", f"+refs/heads/{base}:refs/remotes/origin/{base}",
        f"+refs/heads/{release}:refs/remotes/origin/{release}")


def do_open(api: Api, root: Path) -> int:
    base, release = settings(root)
    slug = slug_of(root)
    fetch(root, base, release)
    if not git(root, "rev-list", f"origin/{release}..origin/{base}"):
        print(f"release: {base} has nothing that {release} does not have; no release PR")
        return 0
    tip = git(root, "rev-parse", f"origin/{base}")
    notes = render_notes(merged_prs(api, root, slug, base, release))
    existing = release_pr(api, slug, release)
    if existing:
        pr = existing[0]
        branch = pr["head"]["ref"]
        tag = branch[len("release/"):]
        # The release branch belongs to this command. Move it to the base tip.
        git(root, "push", "-q", "--force", "origin", f"{tip}:refs/heads/{branch}")
        api("PATCH", f"repos/{slug}/pulls/{pr['number']}", {"body": body_for(tag, notes)})
        print(f"release: moved {branch} to {tip[:12]} and updated PR #{pr['number']}")
        return 0
    tag = next_tag(root)
    branch = f"release/{tag}"
    git(root, "push", "-q", "origin", f"{tip}:refs/heads/{branch}")
    pr = api("POST", f"repos/{slug}/pulls", {"title": f"Release {tag}", "head": branch, "base": release,
                                              "body": body_for(tag, notes)})
    print(f"release: opened PR #{pr['number']} {branch} -> {release}")
    return 0


def ci_state(api: Api, slug: str, sha: str) -> str:
    runs = api("GET", f"repos/{slug}/commits/{sha}/check-runs?per_page=100").get("check_runs", [])
    mine = [r for r in runs if r.get("name") == CI_CHECK]
    if not mine:
        return "missing"
    if any(r.get("status") != "completed" for r in mine):
        return "pending"
    if all(r.get("conclusion") == "success" for r in mine):
        return "success"
    return "failure"


def publish(api: Api, slug: str, tag: str, sha: str, notes: str) -> None:
    try:
        api("GET", f"repos/{slug}/releases/tags/{tag}")
        print(f"release: {tag} already has a GitHub Release")
        return
    except RuntimeError:
        pass
    api("POST", f"repos/{slug}/releases", {"tag_name": tag, "target_commitish": sha, "name": tag, "body": notes})
    print(f"release: created tag and GitHub Release {tag} at {sha[:12]}")


def notes_from_body(body: str) -> str:
    text = re.sub(r"^<!-- starter-release[^\n]*-->\n", "", body or "")
    return re.sub(r"^## Release [^\n]*\n\n", "", text).strip() + "\n"


def tag_from_body(body: str) -> Optional[str]:
    match = re.search(r"<!-- starter-release tag=(v[\d.]+) -->", body or "")
    return match.group(1) if match else None


def catch_up(api: Api, root: Path, slug: str, release: str) -> None:
    """A release PR that merged with no GitHub Release (for example, a merge by hand) gets one."""
    for pr in release_pr(api, slug, release, state="closed")[:5]:
        tag = tag_from_body(pr.get("body") or "")
        if pr.get("merged_at") and pr.get("merge_commit_sha") and tag:
            if git(root, "ls-remote", "--tags", "origin", f"refs/tags/{tag}"):
                continue
            publish(api, slug, tag, pr["merge_commit_sha"], notes_from_body(pr["body"]))


def do_check(api: Api, root: Path) -> int:
    base, release = settings(root)
    slug = slug_of(root)
    catch_up(api, root, slug, release)
    prs = release_pr(api, slug, release)
    if not prs:
        print("release: no open release PR")
        return 0
    pr = api("GET", f"repos/{slug}/pulls/{prs[0]['number']}")
    head = pr["head"]["sha"]
    state = ci_state(api, slug, head)
    if state in ("pending", "missing"):
        print(f"release: PR #{pr['number']} waits for the `{CI_CHECK}` check ({state})")
        return 0
    if state == "failure":
        raise Stop(f"the `{CI_CHECK}` check failed on release PR #{pr['number']}; fix it on {base}, "
                   "then run release.yml again")
    if pr.get("mergeable") is False:
        raise Stop(f"release PR #{pr['number']} has a merge conflict with {release}")
    api("PUT", f"repos/{slug}/pulls/{pr['number']}/merge", {"sha": head, "merge_method": "merge"})
    merged = api("GET", f"repos/{slug}/pulls/{pr['number']}")
    if not merged.get("merged") or not merged.get("merge_commit_sha"):
        raise Stop(f"release PR #{pr['number']} does not show as merged")
    tag = tag_from_body(pr.get("body") or "") or next_tag(root)
    if git(root, "ls-remote", "--tags", "origin", f"refs/tags/{tag}"):
        tag = next_tag(root)
    publish(api, slug, tag, merged["merge_commit_sha"], notes_from_body(pr.get("body") or ""))
    print(f"release: merged PR #{pr['number']} into {release}")
    return 0


def main(argv: Optional[Sequence[str]] = None) -> int:
    parser = argparse.ArgumentParser(description="Release cycle: develop -> release PR -> main, tag, Release.")
    parser.add_argument("mode", choices=("open", "check", "notes"))
    args = parser.parse_args(argv)
    root = Path(git(Path.cwd(), "rev-parse", "--show-toplevel"))
    try:
        if args.mode == "open":
            return do_open(gh_api, root)
        if args.mode == "check":
            return do_check(gh_api, root)
        base, release = settings(root)
        fetch(root, base, release)
        print(render_notes(merged_prs(gh_api, root, slug_of(root), base, release)))
        return 0
    except (Stop, RuntimeError) as error:
        print(f"release: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
