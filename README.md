# Iris

Iris is a local, agent-first email tool. Mail is only the transport. Iris turns
your mail into state: what you owe, what other people owe you, what you know,
what you want to read, and what needs you now. People and agents work on the
same state, and each change to the mailbox is safe.

Read [the Iris manifesto](docs/manifesto.md) for the full model and the plan.

## State

Iris is at release 0 (Foundation). **You cannot use Iris for mail yet.** Release
0 makes the base. Release 1 (See) is the first release that reads a mailbox.

| Part of release 0 | State |
|---|---|
| The `iris` program: `iris --version` and `iris serve` | Done |
| The web client, embedded in the program | Done (an empty shell) |
| Local CI with the memory and start time budget test | Done |
| The synthetic mailbox and the fake Gmail server (`crates/iris-synth`, `crates/iris-fakegmail`) | Done |
| The draft of the open model (`docs/spec/`) | Not yet |

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

You need these tools. Iris builds on macOS and Linux.

- The Rust toolchain in `rust-toolchain.toml`.
- A C compiler. The build compiles SQLite from source.
- Node.js, to build the web client. Node is a build tool only.

1. Build the client, then the program. Run this command from the root of the
   repository:

   ```bash
   (cd client && npx --yes pnpm@10.28.0 install --frozen-lockfile && npm run build) && IRIS_REQUIRE_CLIENT=1 cargo build --release
   ```

2. Run the program:

   ```bash
   target/release/iris --version
   target/release/iris serve
   ```

A plain `cargo build --release` also works. If `client/dist` does not exist, the
program has no web client and serves a fallback page that tells you to build
the client. `IRIS_REQUIRE_CLIENT=1` makes the build stop in that case.
[client/README.md](client/README.md) gives the details of the client build.

`iris serve` listens on `127.0.0.1:4800`. Only this machine can connect to it.

## Connect a Gmail account

Release 1 reads mail only. You need the JSON file of your own Google OAuth
client of the type "Desktop app". The decision record
`docs/decisions/2026-11-google-sign-in.md` tells how to make one.

1. Sign in:

   ```bash
   target/release/iris account add client.json
   ```

2. Open the address that the command prints, and sign in to Google.
3. If the browser is on another machine than Iris, add `--paste`. Then copy
   the address of the browser after the sign-in, and paste it into the
   command.
4. Start the engine:

   ```bash
   target/release/iris serve
   ```

The engine syncs the mailbox at once, then each 60 seconds. If Google refuses
the stored sign-in, run `iris account add` again.

Iris keeps all state in the data folder: `~/Library/Application Support/iris`
on macOS, `~/.local/share/iris` on Linux. `--data <path>` or the variable
`IRIS_DATA` changes the folder. The refresh token and the client file are
encrypted with the `key` file of the data folder.

CAUTION: The backup folder holds the key and your mail. Keep it private.

`iris serve` prints the sign-in address of the local API, with a one-time
code. The web client does not use this address yet (tracker item R1-18). A
program can exchange the code for a token with `POST /api/session`
(`crates/iris-api/README.md`).

## Read the mirror

These commands read the store. Add `--json` to get the JSON of the local API: one JSON document for one
object, and JSON Lines for a list.

| Command | Gives |
|---|---|
| `iris status` | The accounts, the state of the sync and the message counts |
| `iris search "<text>"` | The messages that match the search text, newest first |
| `iris show <id>` | One message (`m-42`) or one item (`c-7`) |
| `iris mirror sync` | One sync of each account now |
| `iris mirror verify` | A compare of the mirror with Gmail, and a check of the content files |

The text of a message is data. `iris show` removes the control characters, so
a message cannot send commands to your terminal.

To copy the data folder while the engine runs, run:

```bash
target/release/iris backup <empty-folder>
```

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
- The product and its users: [PRODUCT.md](PRODUCT.md)
- Plan, design and track work: [docs/PLANNING.md](docs/PLANNING.md)
- Roadmap: [docs/ROADMAP.md](docs/ROADMAP.md)
- Set up and checks: [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)
- Report a security problem: [SECURITY.md](SECURITY.md)
- Code of conduct: [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)

The old Iris code is in a private archive. The new Iris does not use it.

## License

The code is under the Apache License 2.0 (see [LICENSE](LICENSE)). The text of
the open model is under CC BY 4.0. The schemas of the open model use
Apache-2.0.
