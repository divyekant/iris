# iris: rules for agents

These rules apply to every agent that works in this repository (Claude, Codex, and
others). `CLAUDE.md` imports this file. The process comes from the projectstarter
kit (projectstarter 0.6.1). Product rules are at the end, in [Product rules](#product-rules).

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

1. Find or add the board card of the change (see [Plan and track](#plan-and-track)).
   Decide if the change needs a spec. Write a spec only for a decision that is
   architectural, ambiguous, irreversible or risky (external writes, auth,
   personal data, migrations, deletes). Copy `docs/templates/spec.md` to
   `docs/specs/<YYYY-MM-DD>-<name>.md`. A copy edit, a one-line fix or a format
   change needs no spec.

   Rules for a spec:
   - For each external write (a call that changes data outside the product),
     state what happens on success, on a definite rejection, on an unknown
     result (a time-out, a lost response or a crash), and on retry.
   - For each assumption, name the evidence (a command, a file, a document). If
     there is no evidence, write `pending` and say how the design tolerates a
     wrong assumption.
   - Put a question that the build can answer in `## Open questions`, not in
     the design.
   - Give each card its own spec PR. If a spec PR is larger than about 1,500
     changed lines (lockfiles excluded), split it.
   - For input that the product does not control, for concurrency and for crash
     recovery, write the failure list: the supported inputs, each failure mode,
     and one safe result for all other input.

   Keep a code PR to one subsystem and to fewer than about 800 changed lines
   (tests and lockfiles excluded). If the card needs more, split the card first.
2. Start the branch from `develop`. If the branch has no commits of its own:
   `git fetch origin develop` and `git checkout -B <branch> origin/develop`.
3. Write a test that fails without the change. Then write the change. If no test
   can apply (copy, docs), say why in the PR. If the change has a spec, do the
   spec checks of `docs/PLANNING.md` first. If the code goes another way than
   the spec (drift), fix the spec in the same PR.
   - Before you write a parser, a protocol client, a format converter or
     security code, look for a maintained public library. Write your own code
     only if no library fits, or if a library breaks a product rule. Name the
     libraries that you checked, and the reason for the choice, in the PR.
   - Give each parser of input that the product does not control a property
     test or a fuzz test. One test for each example input is not enough.
4. Commit in logical parts. Put a dependency change in its own commit.
   Self-review before the PR:
   - For each new test: what would make it go red? If nothing in the change can,
     it proves nothing.
   - For each new guard or limit: what bypasses it? Find each caller that can
     reach the same place, and say if a test covers it.
   - For each claim in the PR, the spec or the docs: does the same diff refute
     it?
   - When you clear a risk, write the case that you checked, not "checked".

   Pre-review: if the branch adds more than `review.pre_review_lines` lines
   (default 300), run `python3 scripts/review/review.py --pre`. It runs the
   reviewer on the branch and posts nothing. Fix each P0, P1 and P2 finding
   with the steps of "Fix the class" below. Local CI refuses the proof commit
   of such a branch with no pre-review.
5. Run local CI: `scripts/local-ci/run.sh`. It tests each area that the branch
   changes and pushes an empty proof commit. If it fails, fix the cause, commit
   and run it again. Do not open the PR before the proof commit is pushed.
6. Open the PR into `develop` as a draft, with the template
   (`.github/pull_request_template.md`). Fill in `## Summary` (with
   `Card: <ID>` (for example `Card: R0-09`)), `## Verification`,
   `## Known limits and deferred findings` and `## Release Notes`. Move the
   card to `doing` if it is not there yet.
7. Run the review loop:
   `python3 scripts/review/review.py --pr <number>`. It does not merge.
   - Exit 0: the review is clean. Then do these steps:
     1. File the issues of the clean review: each P3 finding that you do not
       fix and each residual risk that names a limit of the product, not of
       the review run (see
       [Known limits and deferred findings](#known-limits-and-deferred-findings)).
     2. List their numbers in one PR comment. Do not edit the PR body, because
       a new body needs a new review.
     3. Run `python3 scripts/review/review.py --pr <number> --merge`. It uses
       the clean review of the same head and body, and merges. The work is
       done.
   - Exit 2, review blocked: fix each P0, P1 and P2 finding with the steps of
     "Fix the class" below. Run local CI, then run the review again.
   - Exit 2, stopped for another reason: act on the reason, then run the review
     again.
   - Exit 1: an error (for example, Codex is not signed in or is out of quota).
     Leave the PR as a draft and tell the user.
8. Stop the loop and tell the user when five review rounds end with no clean
   review. `review.py` stops by itself after `review.max_rounds` blocked rounds
   (default 5). Use `--force` only after the user says to continue.

   After the first round, a P2 finding on code that did not change since the
   last reviewed head does not block. The review comment marks it
   `P2 (older code)`. File one issue for each of these findings (see
   [Known limits and deferred findings](#known-limits-and-deferred-findings)).

   A PR that changes a file in `review.spec_paths` (default `docs/spec/**` and
   `docs/specs/**`) gets the spec rules of `scripts/review/spec.md`. Under these
   rules, an edge case that the build can settle is P3.
9. While the review runs, do not comment on the PR and do not edit its body. A
   change makes the review stale, and the command posts nothing.

P3 findings are advice. They do not block the merge. If you do not fix a P3
finding, file an issue for it.

### Known limits and deferred findings

Each known limit and each finding that a PR does not fix gets one GitHub
issue before the PR merges. This applies to:

- A known limit: a case that the change does not handle on purpose. Examples:
  a size limit, a short built-in list, a `shortcut:` comment, a "Known
  limits" entry in a spec, a residual risk in the clean review.
- A deferred finding: a P3 finding that you do not fix, a `P2 (older code)`
  finding, and a follow-up that the PR names.

Do these steps:

1. Search the open issues first. Do not file a duplicate.
2. Give the issue the label `limit` or `deferred`. Start the title with
   `limit(<area>):` or `deferred(<area>):`.
3. In the body, write what the limit or the finding is, its effect, why the PR
   leaves it, when to change it, and `Source: #<PR>`.
4. List the issue numbers in `## Known limits and deferred findings` of the
   PR body before the review. If there is none, write `None.` List the issues
   of the clean review in a PR comment (step 7).

The issues record limits and findings only. The board stays the tracker of
the work. A card that fixes a limit names its issue.

### Fix the class

A finding names one input that fails. Do not patch that input. Do these steps:

1. Name the invariant that the finding breaks, in one sentence. Example: "The
   kind of a sender always agrees with the evidence in the store."
2. Find each path that can break the same invariant. Use `grep` on the callers.
   The path that the finding names is only one of them.
3. Commit a test of the invariant that fails now. Use a property test where
   possible, not one more example input.
4. Change the code so that the invariant holds by construction. Do not add one
   more special case.
5. Write the invariant, the paths and the test in the message of the fix commit.

If a second finding breaks the same invariant, or `review.py` prints a repair
signal (two rounds in a row found a defect in the fix code), the last fix was a
patch. Redesign that part. Do not patch one more case.

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

## Plan and track

`docs/PLANNING.md` gives the full process. The short rules:

- The board is the only tracker. It is a claude.ai artifact. Its URL is `board`
  in `starter.json`. Do not keep a to-do list or a status table in a file, and
  do not use GitHub Issues or another tool as a tracker. GitHub Issues hold
  only known limits and deferred findings (see
  [Known limits and deferred findings](#known-limits-and-deferred-findings)).
- Each change has a card, except a small fix that the user asks for in the
  session. Each card gets one PR. Iris uses its own tracker (the "Iris tracker"
  artifact), not the kit board. Its columns are `todo`, `doing`, `blocked` and
  `done`. Move the card as the work moves: `doing`, then `done` when the PR
  merged.
- When you need the owner, move the card to `blocked`.
- Build only a card that has `doneWhen` and whose `dependsOn` cards are `done`.
- For each change that the owner can see on a screen, publish a before/after
  page (`docs/board/compare.html`) and let the owner pick. Do not merge before
  the owner picks.
- Before a design or a spec, read `PRODUCT.md`. Before a change to a screen, read
  `docs/DESIGN.md`. If the change adds a token or a UI part, add it to
  `docs/DESIGN.md` in the same PR.
- Log each large decision in `docs/decisions.md`.
- `docs/ROADMAP.md` gives the order of the large pieces of work. Change it only
  when the order changes.

## Project threads

A single change needs no plan: one thread, one card, one PR.

For work with two or more items:

1. A planning thread writes the spec and adds one card for each item. Each card
   has `doneWhen` and `dependsOn`.
2. Do not run two items that change the same files at the same time.
3. Each thread works on one card, and names the card in its PR. An item is
   done only when its PR merged.
4. When each card is in `done`, the planning thread checks the result against
   the spec and reports once.

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
| `crates/iris-synth/` | Test data: a deterministic synthetic mailbox of all five forms, with hostile samples. | `engine` |
| `crates/iris-fakegmail/` | Test server: a fake Gmail API on top of the synthetic mailbox. No network. | `engine` |
| `crates/iris-gmail/` | A thin Gmail API v1 client for the mirror: paging, backoff, quota and a streamed raw fetch. | `engine` |
| `crates/iris-model/` | The model provider layer: one call sends mail text as data to a hosted model (Anthropic) or a local OpenAI-compatible server, and gives JSON, tokens and cost. | `engine` |
| `crates/iris-auth/` | Google sign-in: the client file of the user, the loopback redirect with PKCE, the token exchange and the token refresh. The access token stays in memory. | `engine` |
| `crates/iris-eval/` | Test set: scores an understanding function against the truth of the synthetic mailbox (precision and recall). | `engine` |
| `crates/iris-senders/` | The sender axis of spec 1 section 6.7: decides if the sender of a message is you, a known person, a known service, a stranger or hostile, and keeps the `sender` rows current. Fixed rules, no model. | `engine` |
| `crates/iris-store/` | The store: the data folder, SQLite with full-text search, raw message files and encrypted secrets. | `engine` |
| `crates/iris-people/` | People: one profile for each known person (names, addresses, first and last contact, counts, threads), from the mail history in the store. | `engine` |
| `crates/iris-talk/` | Conversations: the conversation of each mail thread, from the store, as an item of the open model. | `engine` |
| `crates/iris-sync/` | The first sync of the mirror: a full copy of a Gmail mailbox into the store, with resume after a stop. | `engine` |
| `crates/iris-clean/` | The clean text of one message: MIME (through `mail-parser`), charsets, HTML to Markdown, no quotes, signatures or tracking. Pure functions. | `engine` |
| `crates/iris-understand/` | Understanding: one message to items of the five forms. Headers, hostile marks, fixed rules, then the model, and a schema check of each item. `store::understand` runs the steps on one message of the store, with the sender axis and the conversation rule. | `engine` |
| `crates/iris-api/` | The local API: JSON from the store for the web client and the read verbs, and the policy changes of the web client. It refuses requests that are not local. | `engine` |
| `crates/iris-policy/` | The policy file of spec 2 section 6: reads and checks `policy.toml`, and decides each action and the attention of each item, with a reason. Keeps the active policy in `iris.db` and confirms a changed file. Writes the attention of each item after each change of the active policy, and after each change of a fact that a rule reads. Writes the file only for a change from the web client, and keeps its comments. | `engine` |
| `crates/iris-changeset/` | The changeset of spec 2 section 5: drafts, submit (targets to messages and items, flags), the states of each action, the approval hash and the JSON document. No provider client. | `engine` |
| `crates/iris-apply/` | The apply of an approved changeset action to Gmail (`archive`, `inbox`, `read`, `unread`, `trash`), with the mirror update. | `engine` |
| `crates/iris-observe/` | Observe mode: the triage changeset from the forms, with a count of each rule. It applies nothing. | `engine` |
| `crates/iris-triage/` | Triage: stores the proposal of observe mode as a changeset of the author `triage`, one for each account, with a new revision only when the proposal changes. No provider client and no model call. | `engine` |
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
