# Development

This file tells you how to set up iris on a local machine and how to run
its checks. For the rules of the workflow, read `AGENTS.md`.

## Requirements

- git 2.30 or later
- Python 3.9 or later (local CI uses `python3`)
- The tools of each area in `starter.json` (see the `tools` lines)
- `gh`, signed in to GitHub, for the review command
- `codex`, signed in, for the review command

## Set up

1. Clone the repository and go to the `develop` branch:

   ```bash
   git clone https://github.com/divyekant/iris.git
   cd iris
   git checkout develop
   ```

2. Turn on the pre-push hook:

   ```bash
   git config core.hooksPath scripts/git-hooks
   ```

3. Run the setup of each area (the same step as the cloud warm-up):

   ```bash
   python3 scripts/local-ci/local_ci.py --prepare
   ```

## Areas

`starter.json` divides the repository into areas. Each area has these keys:

| Key | Meaning |
|---|---|
| `paths` | Glob patterns of the files of the area. `**/` is any folders. `*` is any name. |
| `setup` | One shell command that installs the tools and packages. It runs again only when a file in `setup_inputs` or a tool version changes. |
| `setup_inputs` | The files that the setup reads, for example lock files. |
| `check` | One shell command for the fast checks (lint, type check). The pre-push hook runs it. |
| `test` | One shell command for the tests. |
| `tools` | Commands that print the tool versions. A new version makes the area run again. |
| `env` | Optional. Variables for the commands of the area. A value that starts with `~/` is a path in your own `HOME`. A value that starts with `./` is a path in the checkout. Use it for a tool that keeps its files in `HOME`, for example `RUSTUP_HOME`. Never put a secret here. |

A change to `starter.json` or to `scripts/local-ci/` selects every area. A
change that matches no area (for example, a docs change) runs no area. It still
gets a proof commit.

All commands run in the repository root with a clean environment: a private
`HOME` in `.local-ci/home`, no tokens, `TZ=UTC` and `LANG=C.UTF-8`. The
`env` of the area is added to this environment.

## Budgets

The test `crates/iris/tests/budget.rs` starts the real `iris serve` program. It
measures two values:

| Value | Limit | How the test measures it |
|---|---|---|
| Start time | 1 second | From the program start to the first HTTP 200 answer |
| Resident memory (RSS) | 50 MB | `ps -o rss= -p <pid>`, on macOS and Linux |
| RSS after a first sync | 50 MB | The same, after `iris serve` syncs 2,000 synthetic messages from the fake Gmail |

If a value is over its limit, the test fails. The limits are the constants
`MAX_START` and `MAX_RSS_KIB` at the top of the file. `cargo test` runs the
test, so local CI runs it too.

To see the measured values, run:

```bash
cargo test -p iris --test budget -- --nocapture
```

Release 0 measures an empty program. When the store exists, the test will also
load 100,000 messages.

Do not increase a limit to make a change pass. If a change needs more memory,
say how much in the PR.

## Local CI

1. Commit your change.
2. Run local CI:

   ```bash
   scripts/local-ci/run.sh
   ```

The command refuses a working tree with changes, a detached head and the
branches `develop`, `main` and `release/*`. It runs `setup`, `check` and `test`
for each changed area, then pushes an empty proof commit to your branch. The
logs are in `.local-ci/logs/<area>.log`.

If an area has the same files and tools as a pass that the branch recorded
before, local CI does not run it again.

Options:

| Option | Effect |
|---|---|
| `--no-proof` | Run the areas. Write and push no proof commit. |
| `--all` | Run every area, not only the changed areas. |
| `--verify <commit>` | Check that a commit is a valid proof commit. |

CAUTION: local CI runs the code of the branch with your user. It removes the
tokens from the environment, but it is not a sandbox. Run it only on branches
of people who can write to the repository.

## Review

```bash
python3 scripts/review/review.py --pr <number> --merge
```

The command runs Codex on the exact head of the PR, in a read-only sandbox, and
posts the result on the PR. If the result is clean, it merges the PR. Read
`AGENTS.md`, "Workflow for one change", for what to do with each result.
Add review rules of the product to `scripts/review/rules.md`.

Two settings in `review` of `starter.json` control the loop:

| Setting | Default | Effect |
|---|---|---|
| `max_rounds` | `5` | The command stops after this number of blocked rounds. `--force` continues. |
| `spec_paths` | `["docs/spec/**", "docs/specs/**"]` | A PR that changes a matching file gets the spec rules of `scripts/review/spec.md`. |
