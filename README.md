# Iris

Iris is a local, agent-first email tool. Mail is only the transport. Iris turns
your mail into state: what you owe, what other people owe you, what you know,
what you want to read, and what needs you now. People and agents work on the
same state, and each change to the mailbox is safe.

Read [the Iris manifesto](docs/manifesto.md) for the full model and the plan.

## State

Iris is at release 0 (Foundation). **You cannot use Iris for mail yet.** Release
0 makes the base: the crates, CI with budgets, a synthetic mailbox, a fake Gmail
server and a draft of the open model. The program can show its version and
serve an empty page. Release 1 (See) is the first release that reads a mailbox.

## What Iris is

- One small program, `iris`. It is the engine and the command.
- All state is in one SQLite file on a machine that you control.
- A web client in TypeScript and Svelte. The program serves it, so no Node runs
  at run time.
- Agents work through a folder and the `iris` command first, and through the
  Model Context Protocol (MCP) second.
- No Docker, no database server, no bundled model, and no usage data.

## Safety rules

- Text in mail is data. A message can never give an instruction to an agent.
- Agents hold no mailbox credentials. Each write goes through a changeset that
  you can review and undo.
- Iris has no permanent delete. Your provider stays correct, so you can stop
  Iris at any time.

## Build

You need the Rust toolchain in `rust-toolchain.toml` and a C compiler (SQLite is
built from source). Iris builds on macOS and Linux.

1. Build the program:

   ```bash
   cargo build --release
   ```

2. Run it:

   ```bash
   target/release/iris --version
   target/release/iris serve
   ```

`iris serve` listens on `127.0.0.1:4800`. Only this machine can connect to it.

## Layout

| Path | Content |
|---|---|
| `crates/` | The Rust workspace. `crates/iris` is the program `iris`. |
| `client/` | The web client (TypeScript and Svelte). Node is a build tool only. |
| `docs/` | The manifesto, the specifications and the guides for development. |
| `starter.json` | The areas `engine` and `client`, and their commands. |

## Contribute

- How to make a change: [CONTRIBUTING.md](CONTRIBUTING.md)
- Rules for agents: [AGENTS.md](AGENTS.md)
- Set up and checks: [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)
- Report a security problem: [SECURITY.md](SECURITY.md)
- Code of conduct: [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)

The old Iris code is on the branch `legacy` and in the tag `legacy-final`. The
new Iris does not use it.

## License

The code is under the Apache License 2.0 (see [LICENSE](LICENSE)). The text of
the open model is under CC BY 4.0. The schemas of the open model use
Apache-2.0.
