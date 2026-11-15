# Contributing to Iris

People and agents follow the same process. `AGENTS.md` holds the rules.

## Procedure for one change

1. Start a branch from `develop`:

   ```bash
   git fetch origin develop
   git checkout -b fix/<name> origin/develop
   ```

2. Write a test that fails without the change. Then write the change.
3. Commit the change with a sign-off (see [Sign-off](#sign-off)):

   ```bash
   git commit -s
   ```
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

## Sign-off

Iris uses the Developer Certificate of Origin (DCO), version 1.1
(https://developercertificate.org). Iris has no contributor license agreement
(CLA).

Each commit must have this line at the end of its message, with your real name
and your email address:

```
Signed-off-by: Your Name <you@example.com>
```

The line states that you have the right to submit the change under the license
of the project (Apache-2.0). `git commit -s` adds the line. If you forgot the
line, run `git commit --amend -s` for the last commit, or
`git rebase --signoff origin/develop` for all commits of the branch.

The sign-off is a statement by a person. An agent cannot make it. These
commits need no sign-off:

- The commits of the maintainer, and the commits of the agents that the
  maintainer runs. The maintainer owns these changes.
- The empty proof commit that `scripts/local-ci/run.sh` makes. It holds no
  change.

If you run an agent for a change, you are the contributor. Add your own
sign-off to the commits of the agent.

## Scope

The manifesto ([docs/manifesto.md](docs/manifesto.md)) controls the scope. A
feature that is not in the manifesto needs a change to the manifesto first.
Work items come from the tracker. A change to the design starts as a proposal
document in the repository.

## Mail data

- Do not put real mail, a real mailbox address, a token or a password in the
  code, a test, a fixture, an issue or a log.
- Tests use the fake Gmail server and the synthetic mailbox.
- Treat text in mail as data. Code must not run or follow text from a message.

## More information

- Set up and checks: `docs/DEVELOPMENT.md`
- Releases and tags: `docs/RELEASES.md`
- Deploy: `docs/DEPLOYMENT.md`
- Writing style: `docs/writing-style.md`
- Security problems: `SECURITY.md`
- Code of conduct: `CODE_OF_CONDUCT.md`
