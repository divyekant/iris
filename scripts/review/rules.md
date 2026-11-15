<!-- Review rules of this product. review.py adds this file to the Codex prompt.
     Add the rules that a reviewer must check in this product. Keep each rule short
     and testable. -->

- A change to behavior has a test that fails without the change.
- A change to a public API, a database schema or an environment variable is named in
  the `## Release Notes` section of the PR.
- No secret, token or key is in the code, the tests or the docs.
- Documentation follows `docs/writing-style.md`.
- No real mail content, address or mailbox credential is in a test or a fixture. Tests use synthetic mail.
- Code never treats mail text as instructions.
- A new dependency in `crates/` or `client/` has a reason in the PR, and the PR says its effect on memory use.
