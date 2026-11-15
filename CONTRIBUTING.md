# Contributing to iris

People and agents follow the same process. `AGENTS.md` holds the rules.

## Procedure for one change

1. Start a branch from `develop`:

   ```bash
   git fetch origin develop
   git checkout -b fix/<name> origin/develop
   ```

2. Write a test that fails without the change. Then write the change.
3. Commit the change.
4. Run local CI:

   ```bash
   scripts/local-ci/run.sh
   ```

5. Open a draft PR into `develop`. Fill in the template, with the
   `## Release Notes` section.
6. Run the review and the merge:

   ```bash
   python3 scripts/review/review.py --pr <number> --merge
   ```

7. If the review is blocked, fix each P0, P1 and P2 finding. Then go back to
   step 3.

Do not merge a PR with the GitHub button or `gh pr merge`.

## More information

- Set up and checks: `docs/DEVELOPMENT.md`
- Releases and tags: `docs/RELEASES.md`
- Deploy: `docs/DEPLOYMENT.md`
- Writing style: `docs/writing-style.md`
