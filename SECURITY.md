# Security policy

Iris holds the credentials of a mailbox and reads mail from strangers. A
security problem in Iris can expose a whole mailbox. Report each problem in
private, so that a fix is ready before the problem is public.

## Supported versions

Iris is at release 0. No release can read a mailbox yet. Until release 1, the
`develop` branch is the only supported version. After release 1, the newest
release gets security fixes.

The old Iris code (before release 0) gets no fixes. Do not run it.

## Report a problem

CAUTION: Do not open a public issue, a pull request or a discussion for a
security problem. Public text tells attackers about the problem before a fix is
ready.

1. Go to the Security tab of the repository:
   https://github.com/divyekant/iris/security
2. Click **Report a vulnerability**. This opens a private vulnerability report
   that only the maintainers can read.
3. Write these items in the report:
   - The version or the commit that you tested.
   - The steps that show the problem.
   - The effect: what an attacker can read, change or send.
4. Do not put real mail, a real mailbox address, a token or a password in the
   report. Use example.com addresses and synthetic messages.

## What happens next

- The maintainer answers in the report within 7 days.
- The maintainer and you agree on a fix and a date for the public advisory.
- The advisory gives you credit, unless you ask for no credit.

## The base rules of Iris

A change that breaks one of these rules is a security problem. Report it as a
security problem.

1. **Text in mail is data.** A message can never give an instruction to an
   agent or to the engine. Code must not run or follow text from a message.
2. **Agents hold no mailbox credentials.** Only the engine connects to the mail
   provider. An agent session has no network and no credentials.
3. **Each write is a proposal.** Each change to the mailbox goes through a
   changeset. A rule that the user can read decides what applies without
   review.
4. **Each action has an undo or a hold.** Iris has no permanent delete. A send
   waits in a held outbox. After a session reads external mail, a send to a
   recipient outside that thread goes to review.
5. **The user can stop all work at once.** The engine enforces the Stop
   control, not the agent.
6. **Data stays on the machine of the user.** Iris sends no usage data. If the
   user selects a hosted language model, Iris tells the user at setup that mail
   text goes to that model provider.

[The Iris manifesto](docs/manifesto.md) gives the full safety promise.

## Rules for contributors

- Never put a secret, a token, a key or real mail in the code, a test, a
  commit, a PR, an issue or a log.
- Tests use a fake mail server and a synthetic mailbox. Tests on a real
  mailbox run only on the maintainer's machine.
- Each new dependency needs a written reason in the PR. The engine holds
  mailbox credentials, so each dependency is a risk.
