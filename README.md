# Iris

Iris is a local, agent-first email tool. Mail is only the transport. Iris turns
your mail into state: what you owe, what other people owe you, what you know,
what you want to read, and what needs you now. People and agents work on the
same state, and each change to the mailbox is safe.

Read [the Iris manifesto](docs/manifesto.md) for the full model and the plan.

## State

Iris reads, understands and changes a Gmail, Outlook or IMAP mailbox. The
releases of the manifesto, "The path", are built on the `develop` branch. The
GitHub Releases page gives the newest release.

These checks are open before daily use:

| Check | State |
|---|---|
| Send. Iris sends no mail until you turn sending on with `iris sending on`. | Open: a send check on a real mailbox (issue #302). |
| The Outlook and IMAP mirrors on a real mailbox for 7 days. | Open. |
| The Gmail mirror on a real mailbox for 7 days. | Done: 0 differences. |

## Install

Use this command on macOS or Linux. It installs the newest release to
`~/.local/bin/iris` and starts the background service with
`iris service install`:

```bash
curl -fsSLO https://raw.githubusercontent.com/divyekant/iris/main/scripts/install.sh && sh install.sh
```

[docs/DEPLOYMENT.md](docs/DEPLOYMENT.md), "Install Iris", gives each step of the
script and the data that it sends. If the script says that
`~/.local/bin` is not on your `PATH`, add it as the script shows. Then connect
an account (see below) and run `iris open`.

To build from source, see "Build".

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

The other sections of this README run `iris`. In a source build, use
`target/release/iris`, or copy it to a folder on your `PATH`.

A plain `cargo build --release` also works. If `client/dist` does not exist, the
program has no web client and serves a fallback page that tells you to build
the client. `IRIS_REQUIRE_CLIENT=1` makes the build stop in that case.
[client/README.md](client/README.md) gives the details of the client build.

`iris serve` listens on `127.0.0.1:4800`. Only this machine can connect to it.
To use a phone, see "Open Iris on a phone".

## Connect a Gmail account

You need the JSON file of your own Google OAuth client of the type "Desktop
app". The decision record `docs/decisions/2026-11-google-sign-in.md` tells how
to make one.

By default Iris reads mail only (scope `gmail.readonly`). To let Iris apply the
changes and the sends that you approve, give it write access:

1. In the Google Cloud console, open Google Auth Platform, then Data Access.
2. Click "Add or remove scopes", add `gmail.modify`, and save.
3. Add `--write` to the sign-in command below.

A read-only account syncs as before. Its approved changes wait, and
`iris status` shows `read_only`. Run the sign-in with `--write` again at any
time: the account keeps its copy of the mail.

1. Sign in:

   ```bash
   iris account add client.json
   ```

   For write access: `iris account add client.json --write`.

2. Open the address that the command prints, and sign in to Google.
3. If the browser is on another machine than Iris, add `--paste`. Then copy
   the address of the browser after the sign-in, and paste it into the
   command.
4. Start the engine:

   ```bash
   iris serve
   ```

The engine syncs the mailbox at once, then each 60 seconds. If Google refuses
the stored sign-in, run `iris account add` again.

## Connect an Outlook account

You need an app registration of your own in Microsoft Entra. The decision
record `docs/decisions/2026-11-microsoft-sign-in.md` tells how to make one.

1. Sign in with the application (client) ID of the registration:

   ```bash
   iris account add-outlook <client ID>
   ```

2. Open the address that the command prints, and sign in to Microsoft. For
   a single-tenant registration, add `--tenant <tenant ID>`. For a browser
   on another machine, add `--paste`.
3. Start the engine with `iris serve`.

Microsoft gives a new refresh token at each refresh. Iris encrypts each new
token in place of the old one. If Microsoft refuses the stored sign-in, run
`iris account add-outlook` again.

## Connect an IMAP account

Iris reads an IMAP mailbox over TLS. The server must support `CONDSTORE`.

```bash
iris account add-imap <address> --host <server>
```

The command asks for the password with no echo, signs in one time to check
it, and saves it encrypted. Use an app password if your provider has them.
`--port` (default 993) and `--user` (default the address) change the sign-in.
To give a new password, run `iris account add-imap <address>` with no
`--host`. Then start the engine with `iris serve`.

An IMAP account has these limits: Iris can change the read state of a
message, and add or remove a keyword that the server already lists. It cannot
archive, move, trash, draft or send (issues #775 and #628), and it makes no new
keyword (issue #652).

## Data folder and the web client

Iris keeps all state in the data folder: `~/Library/Application Support/iris`
on macOS, `~/.local/share/iris` on Linux. `--data <path>` or the variable
`IRIS_DATA` changes the folder. The refresh token and the client file are
encrypted with the `key` file of the data folder.

To open the web client, run `iris open`. It asks the running engine for a
new sign-in address with a one-time code, and opens it in the browser. The
address works one time, for 2 minutes. `iris open --print` prints the address
and opens nothing. `iris serve` also prints one sign-in address at start. A
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

## Changesets from the terminal

These commands talk to the running engine through the socket `iris.sock` in
the data folder. They do not read the store. Start the engine with
`iris serve` first. With no engine, they exit with code 4.

| Command | Does |
|---|---|
| `iris changeset <cs-id>` | Shows a changeset and the state of each action. Add `--json` for the document. |
| `iris changeset --drafts` | Lists your draft changesets, newest first. |
| `iris withdraw <cs-id>` | Withdraws a changeset that the command made. Applied actions stay. |
| `iris close <c-id>` | Adds `close` of a commitment to a draft changeset. `--reason` is optional. The reason has at most 500 characters: submit denies a longer reason. |
| `iris decline <c-id>` | Adds `decline` of a commitment to a draft changeset. `--reason` is optional. The reason has at most 500 characters: submit denies a longer reason. |
| `iris skip <f-id>`, `iris mute <id>`, `iris dismiss <s-id>` | Adds `skip` of a feed item, `mute` of a conversation or `dismiss` of a signal to a draft changeset. `--reason` is optional, with the same limit as `close`. |
| `iris defer <c-id> --date <YYYY-MM-DD>` | Adds `defer` of a commitment to a draft changeset. |
| `iris correct <id> --field <path> --value <json>` | Adds a correction of one field of an item to a draft changeset. |
| `iris archive`, `inbox`, `read`, `unread`, `trash <ids>` | Adds the mail action on threads (`t-3`) or messages (`m-42`) to a draft changeset. |
| `iris label`, `unlabel <ids> --label <name>` | Adds or removes a label in a draft changeset. |
| `iris submit <cs-id> --why <text>` | Submits the draft. The policy decides each action. Prints the state of each action. |

Each add command makes a new draft changeset and prints its ID. Add
`--changeset <cs-id>` to add to a draft that exists, and `--why <text>` to
give the reason of the action. A draft does nothing until you submit it.

`iris submit` exits with one of these codes:

| Code | Meaning |
|---|---|
| 0 | Each action of the new revision is applied. |
| 5 | One or more actions are open: they wait for your approval in the web client, or for the apply. |
| 3 | The policy or a hard limit denied one or more actions. |
| 2 | The request is not correct, or the engine refused an action, for example a wrong target. |
| 1 | An error. Run `iris changeset <cs-id>` to see the state. |

## Agents through MCP

`iris mcp` serves the same verbs as MCP (Model Context Protocol) tools, on
standard input and output. Use it with an agent that speaks MCP but does not
use a terminal. Each tool has the name and the arguments of its verb:
`status`, `search`, `show`, `draft`, `send`, `close`, `decline`, `skip`,
`mute`, `dismiss`, `defer`, `correct`, `archive`, `inbox`, `read`, `unread`, `trash`, `label`,
`unlabel`, `submit`, `changeset`, `drafts`, `withdraw`, `revise`,
`watch_add`, `watch_list`, `watch_remove`, `matters_list` and
`matters_show`. The result is what the verb prints with `--json`, or its
text where the verb has no `--json` (`watch_add` and `watch_remove`). A tool
that adds an action gives the JSON `{"changeset": ..., "n": ...}`.

The tools take the same path as the command, with the same author (`local`),
the same policy and the same review. The read tools and the watch tools use
the store directly. The other tools need the engine: start `iris serve`
first. A failed tool call
gives the error line and the exit code of the command. MCP has no tool to
approve, to deny or to change the policy. Only the web client does these
steps.

Add Iris to the MCP settings of the agent. For example:

```json
{ "mcpServers": { "iris": { "command": "iris", "args": ["mcp"] } } }
```

The sync stores the raw messages only. After each sync, `iris serve` and
`iris mirror sync` run the understanding step, so words and the `from:` and
`to:` filters find the new mail. The free steps run with no model. Paid model
calls run only inside an approved budget (see "Understand the mail").

The text of a message is data. `iris show` removes the control characters, so
a message cannot send commands to your terminal.

## Agents that do a job

A job gives one goal and its threads to an agent (release 4). To make a job,
open an item in the web client, press "Handle this", and type the goal. The
address of the job page ends with the job ID, for example `#/jobs/j-1`. Then start a session of an
agent with this command:

```bash
iris job run --agent claude-code <job ID>
```

`iris job session <job ID>` shows the state of the last session. Its log is
`sessions/<session>.log` in the data folder. The session runs in a sandbox
with only the workspace of the job and no login file of yours. It gets only
the variables and the hosts of its adapter:

| Agent | Program | Variables | Hosts |
|---|---|---|---|
| `claude-code` | `claude` | `ANTHROPIC_API_KEY`, or `CLAUDE_CODE_OAUTH_TOKEN` from `claude setup-token` | `api.anthropic.com` |
| `codex` | `codex` | `OPENAI_API_KEY`, `CODEX_HOME` | `api.openai.com` |

The program must be on the `PATH` of the engine. The variables come from the
environment of the engine (`iris serve` or the service), not from your
terminal. If a variable is missing, the agent cannot sign in and the session
ends with an error.

To add another agent tool, or to change the time limit of a session (30
minutes by default), write `agents.toml` in the data folder:

```toml
[agent.my-agent]
command = ["my-agent", "--run"]
pass_env = ["MY_AGENT_KEY"]
allow_hosts = ["api.example.com"]

[session]
max_minutes = 60
```

Iris adds the fixed prompt as the last argument of `command`.

On Linux, the sandbox needs `bwrap`. [docs/DEPLOYMENT.md](docs/DEPLOYMENT.md)
gives the prerequisites of the sandbox on Linux and on macOS.

## Open Iris on a phone

The web client has a phone layout. The engine listens only on a loopback
address (`127.0.0.1:4800`), because the API has no TLS. `iris serve --addr`
refuses an address that is not a loopback address.

CAUTION: Do not make port 4800 available on a network with a proxy or a port
forward. The session token and your mail go without encryption.

Use an SSH tunnel from the phone to the machine that runs Iris:

1. Install an SSH client with port forward on the phone.
2. Forward local port 4800 of the phone to `127.0.0.1:4800` of the Iris
   machine. With the OpenSSH command, the forward is
   `ssh -L 4800:127.0.0.1:4800 <user>@<iris-machine>`.
3. On the Iris machine, run `iris open --print`. It prints a sign-in address
   that works one time, for 2 minutes.
4. Open that address in the browser of the phone while the tunnel is open.

For a phone that is not on your local network, use a private network (for
example a VPN) for the SSH connection. Do not put the SSH port of the
machine on the internet without a key-only login.

## Watch for a mail

A watch tells you one time when a new mail matches a search text.

| Command | Does |
|---|---|
| `iris watch add "<text>"` | Starts a watch, for example `iris watch add "visa from:embassy"`. The text uses the syntax of `iris search`. |
| `iris watch list` | Shows each watch. A watch `waits`, or it `arrived` with the mail that matched. |
| `iris watch remove <id>` | Removes the watch (`w-3`). |

Test the text with `iris search` first. Only mail that arrives after the
watch can match. `iris serve` checks each minute. Mail that you send, drafts
and hostile mail never match.

## Understand the mail

The understanding step reads each message once and stores its items. Fixed
rules run first and cost nothing. A model reads the rest. A message that was
understood is not read again, so a second run makes no model call.

| Command | Does |
|---|---|
| `iris model set local <base-url> <model>` | Uses a model server on this machine, for example `http://localhost:11434/v1`. No key, no cost. |
| `iris model set anthropic <model> --price-in <usd> --price-out <usd>` | Uses Anthropic. Reads the API key from standard input. The prices are USD for each million tokens. |
| `iris model show` | The model, the prices and the budget that is left. It never shows the key. |
| `iris understand --estimate` | The count of messages, the tokens and the cost. It makes no call. It prints an approval code. |
| `iris understand --approve <code> --budget <usd>` | Sets the budget. It needs a terminal and asks you to type the budget again. |
| `iris understand` | Reads the messages that are not understood yet, inside the budget. |

A hosted model makes no call before you approve a budget. Each
`iris model set` clears the budget, so a new estimate and a new approval are
necessary.

To get a draft of a reply in your voice, run:

```bash
iris write t-42 --intent "yes, Friday at 10"
```

The model reads the thread and up to 8 of your sent messages (mail to the
same person first), and writes the reply in their style. The command saves
the draft and prints it. It sends nothing. When you open the reply to the
same thread in the web client (`#/write/t-42`), the reply box shows the saved
draft. If a newer mail came into the thread, the draft does not show. The call uses the same budget as
understanding. Iris makes no draft for a thread with hostile mail, or for a
thread with a message that understanding did not read yet.

## Back up, export and move

CAUTION: A backup folder and an export folder hold your mail. A backup folder
also holds the key. Keep them private.

To copy the data folder while the engine runs, run:

```bash
iris backup <empty-folder>
```

The backup is a complete data folder. Iris opens it as it is, at any path.

To get your data in open formats, run:

```bash
iris export <empty-folder>
```

| File | Content |
|---|---|
| `items.jsonl` | One item on each line, as the open model gives it |
| `messages.jsonl` | One message on each line, as `iris show <id> --json` gives it, with `file`: the path of its raw message |
| `mail/<id>.eml` | The raw message. A mail program can open it. |

The export holds no secret: no key, no sign-in and no model key.

To move Iris to another machine, do these steps:

1. On the old machine, stop the engine. If the background service runs it,
   run `iris service remove`.
2. Run `iris backup <empty-folder>`.
3. Copy the backup folder to the data folder of the new machine:
   `~/Library/Application Support/iris` on macOS, `~/.local/share/iris` on
   Linux. If Iris ran on the new machine before, move its data folder away
   first. Use a private channel, for example `scp -r`.
4. On the new machine, install Iris and run `iris status`. Make sure that
   it shows your accounts.
5. Run `iris service install`.

CAUTION: Do not run the engine on two machines with the same data. Two
engines can apply one change two times.

The sign-in and the OAuth client file move with the backup, so the new
machine needs no new sign-in.

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
