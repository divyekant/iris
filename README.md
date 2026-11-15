# Iris

Iris is a local, agent-first email tool. One small program, `iris`, holds the
engine and the command line. It keeps all state in SQLite on your own machine or
server. A web client in TypeScript and Svelte is built into the program. People
and agents use the same model of your mail.

Iris is open source under the Apache License 2.0 (see `LICENSE`). The old Iris
code is on the branch `legacy` and in the tag `legacy-final`.

## Layout

| Path | Content |
|---|---|
| `crates/` | The Rust workspace. `crates/iris` is the program `iris`. |
| `client/` | The web client (TypeScript and Svelte). Node is a build tool only. |
| `starter.json` | The areas `engine` and `client`, and their commands. |

## Start

- Set up and checks: [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)
- How to make a change: [CONTRIBUTING.md](CONTRIBUTING.md)
- Rules for agents: [AGENTS.md](AGENTS.md)
- Releases: [docs/RELEASES.md](docs/RELEASES.md)
- Deploy: [docs/DEPLOYMENT.md](docs/DEPLOYMENT.md)

This repository uses the projectstarter kit (projectstarter 0.2.0): local CI with a
proof commit, a Codex review loop, and a release cycle from `develop` to `main`.
