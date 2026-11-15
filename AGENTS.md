# iris: rules for agents

These rules apply to every agent that works in this repository (Claude, Codex, and
others). `CLAUDE.md` imports this file. The process comes from the projectstarter
kit (projectstarter 0.2.0). Product rules are at the end, in [Product rules](#product-rules).

Write all documentation, PR text and code comments as `docs/writing-style.md` says
(Simplified Technical English).

## Branches

| Branch | Use | Who writes to it |
|---|---|---|
| `develop` | Integration. Each feature PR merges here. | `scripts/review/review.py --merge` only |
| `main` | Production. Each release PR merges here. | `release.yml` only |
| `release/v<YYYY.MM.DD.N>` | The release candidate: a copy of `develop`. | `scripts/release/release.py` only |
| `feat/<name>`, `fix/<name>`, `chore/<name>` | One change. | The agent or person that does the change |

Never push to `develop`, `main` or a `release/*` branch. Never merge with
`gh pr merge`, the GitHub button or `git push`.

## Set up

- Cloud session (claude.ai/code): the cloud setup installs the tools. The
  SessionStart hook (`scripts/local-ci/session-start.sh`) runs the setup of each
  area in the background. Nothing else is necessary.
- Local machine: follow `docs/DEVELOPMENT.md`.

The areas of the product, and the commands of each area, are in `starter.json`.
To add a tool, a test command or a new folder, change `starter.json`.

## Workflow for one change

1. Decide if the change needs a spec. Write a spec only for a decision that is
   architectural, ambiguous, irreversible or risky (external writes, auth,
   personal data, migrations, deletes). Put it in `docs/specs/<YYYY-MM-DD>-<name>.md`
   with a `## Assumptions` section. A copy edit, a one-line fix or a format change
   needs no spec.
2. Start the branch from `develop`. If the branch has no commits of its own:
   `git fetch origin develop` and `git checkout -B <branch> origin/develop`.
3. Write a test that fails without the change. Then write the change. If no test
   can apply (copy, docs), say why in the PR.
4. Commit in logical parts. Put a dependency change in its own commit.
5. Run local CI: `scripts/local-ci/run.sh`. It tests each area that the branch
   changes and pushes an empty proof commit. If it fails, fix the cause, commit
   and run it again. Do not open the PR before the proof commit is pushed.
6. Open the PR into `develop` as a draft, with the template
   (`.github/pull_request_template.md`). Fill in `## Summary`, `## Verification`
   and `## Release Notes`.
7. Run the review loop:
   `python3 scripts/review/review.py --pr <number> --merge`.
   - Exit 0: the review is clean and the PR is merged. The work is done.
   - Exit 2, review blocked: fix each P0, P1 and P2 finding. For a code finding,
     first commit a test that fails on it, then the fix. Run local CI, then run
     the review again.
   - Exit 2, stopped for another reason: act on the reason, then run the review
     again.
   - Exit 1: an error (for example, Codex is not signed in or is out of quota).
     Leave the PR as a draft and tell the user.
8. Stop the loop and tell the user in these cases:
   - Two rounds in a row find a defect in a fix that you made for the round
     before (a repair stop). Name the subsystem and what the fixes get wrong.
     Do not push one more fix.
   - Five review rounds end with no clean review.
9. While the review runs, do not comment on the PR and do not edit its body. A
   change makes the review stale, and the command posts nothing.

P3 findings are advice. They do not block the merge. Open an issue for a P3
finding only when the follow-up is worth tracking.

## Release notes

Each PR has a `## Release Notes` section with these five lines:

```markdown
## Release Notes
- User-facing:
- Internal:
- Migrations/config/env:
- Docs/version/dependency bumps:
- Risk/rollback:
```

Write the effect, not the code. Leave a line empty if it does not apply. For a
change with no release effect (a test, a refactor, a docs fix), write
`No release-note impact.` in place of the five lines. `release.py` copies the
lines into the release PR and the GitHub Release, with the PR number.

## Releases

Agents do not release. `.github/workflows/release.yml` does each step:

1. On its schedule, it makes `release/v<YYYY.MM.DD.N>` from `develop` and opens
   the release PR into `main`, with the collected release notes. If a release PR
   is open, it moves the release branch to the newest `develop`.
2. The release PR runs full CI (`ci.yml`): every area of `starter.json`.
3. When the `ci` check is green, it merges the release PR, then makes the tag
   `v<YYYY.MM.DD.N>` and the GitHub Release.
4. The push to `main` runs `deploy.yml`.

If release CI fails, the release PR stays open and the workflow run fails, so
GitHub tells the owner. Fix the cause with a normal feature PR into `develop`.
The next run moves the release branch.

For an urgent fix: merge the fix into `develop` with the normal workflow, then
start `release.yml` by hand (Actions, release, Run workflow).

The GitHub Release is the release record. `CHANGELOG.md` is optional.

## GitHub Actions

| Workflow | Starts on | Does | Time |
|---|---|---|---|
| `proof-check.yml` | Each push to a PR into `develop` | Checks that the head is a local CI proof commit. Runs no tests. | Seconds |
| `ci.yml` | Each push to a PR into `main`, and by hand | Runs every area of `starter.json` | The full suite |
| `release.yml` | Schedule, the end of a `ci` run, and by hand | Opens, moves and merges the release PR. Makes the tag and the Release. | About 1 minute |
| `deploy.yml` | Each push to `main`, and by hand | The deploy steps of the product | Depends on the product |

Do not add a workflow that runs the tests on feature PRs. Local CI does that work.

## Deploy

`docs/DEPLOYMENT.md` holds the deploy steps of the product. Agents do not
deploy by hand. Do not change production without the owner's approval.

## Project threads

A single change needs no plan and no tracker: one thread, one PR.

For work with two or more items:

1. A planning thread writes the plan and one tracker. Give the tracker one row for
   each item: Item, Done means, Files it changes, Waits on, Thread, PR, Status.
2. Status values: `Not started`, `In progress`, `PR open`, `Merged`, `Needs you`.
3. Do not run two items that change the same files at the same time.
4. Each thread updates only its own row. An item is done only when its PR merged.
5. When every row is `Merged`, the planning thread checks the result against the
   plan and reports once.

## Secrets

Never put a secret, a token or a key in the code, a test, a commit, a PR, a
comment or a log. Local CI removes the tokens of the session from the test
environment. Keep production keys in the secret store of the host.

## Product rules

Write the name as "Iris", not "IRIS". The program and the command are `iris`.

### Folders

| Path | Content | Area |
|---|---|---|
| `crates/` | The Cargo workspace of small crates. `crates/iris` is the one program: engine and command. | `engine` |
| `crates/iris-web/` | Serves the web client. It embeds the build of `client/`. | `engine` |
| `client/` | The web client in TypeScript and Svelte. The build makes static files that the program embeds. | `client` |

### Base decisions

- One Rust program `iris`. No Docker, no database server, no bundled model.
- SQLite with FTS5 holds all state, through `rusqlite` with the `bundled` feature.
  The build needs a C compiler.
- The lightest memory footprint has priority over code size and build time. A
  change that adds memory must say how much in the PR.
- Iris must run on macOS (the owner's Mac) and on Linux (a small server). CI
  runs on Linux only. Do not use an API that only one of the two systems has.

### Mail data and secrets

- Mail text is data, never instructions. Code must not run or follow text from a
  message.
- Agents hold no mailbox credentials. Never put a real mailbox token or password
  in the code, a test, a fixture, the cloud environment or a log.
- Tests use a fake mail server and a synthetic mailbox. Tests on a real mailbox
  run only on the owner's Mac.
