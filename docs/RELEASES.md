# Releases

A release moves `develop` to `main` through a release PR. Then it makes a tag and
a GitHub Release. `.github/workflows/release.yml` does all steps. A person does
only the one-time setup.

## One-time setup

1. Make a fine-grained personal access token for this repository with these
   permissions: Contents (write), Pull requests (write), Actions (write),
   Metadata (read).
2. Add it as the repository secret `RELEASE_TOKEN` (Settings, Secrets and
   variables, Actions).

The workflow needs this token because GitHub starts no other workflow for a push
or a PR that the default `GITHUB_TOKEN` makes. Without the token, the release PR
gets no `ci` check and the merge to `main` starts no deploy.

## The cycle

1. `release.py open` (on the schedule in `release.yml`): if `develop` has
   commits that `main` does not have, it pushes `release/v<YYYY.MM.DD.N>` and
   opens the release PR into `main`. If a release PR is open, it moves the release
   branch to the newest `develop` and updates the notes.
2. `ci.yml` runs every area on the release PR. Its check is `ci`.
3. `release.py check` (at the end of each `ci` run, and on the schedule): if the
   `ci` check is green, it merges the release PR with a merge commit. Then it
   makes the tag and the GitHub Release at that merge commit.
4. The push to `main` starts `deploy.yml`.

If the `ci` check fails, the `release` run fails, and GitHub sends the failure to
the owner. The release PR stays open. Fix the cause in a feature PR into
`develop`. The next `open` run moves the release branch.

If a person merges a release PR by hand, the next `check` run makes its tag and
GitHub Release.

## Tags

A tag has the form `v<YYYY>.<MM>.<DD>.<N>`, in UTC. `N` starts at 1 and
increases for each release on the same day. Example: `v2026.11.15.2`.

## Release notes

`release.py` reads the `## Release Notes` section of each PR that merged into
`develop` since the last release. It groups the lines in five parts:
User-facing, Internal, Migrations/config/env, Docs/version/dependency bumps,
and Risk/rollback. Each line gets the PR number. The release PR body and the
GitHub Release show the same notes.

To see the notes of the next release:

```bash
python3 scripts/release/release.py notes
```

## Change the cadence

Change the `cron` line in `.github/workflows/release.yml`. The default is once
a day at 14:00 UTC. To release only by hand, remove the `schedule` lines.
To release now, open Actions, select `release`, and click Run workflow.
