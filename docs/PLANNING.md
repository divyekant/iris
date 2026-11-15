# Plan, design and track work

All products of the projectstarter kit use this process. `AGENTS.md` holds the
short rules. This file gives the detail.

Iris uses its own board, the "Iris tracker" artifact (`board` in `starter.json`).
Its cards have `title`, `release`, `part`, `status` (`todo`, `doing`, `blocked`,
`done`), `doneWhen` and `dependsOn`. Card IDs are like `R0-09`. Where this file
names a field or a column of the kit board that the Iris tracker does not have,
follow `AGENTS.md`, "Plan and track".

## The parts

| Part | Where | Use |
|---|---|---|
| Board | A claude.ai artifact. The URL is `board` in `starter.json`. | Each piece of work and its status. The only tracker. |
| Before/after | One claude.ai artifact for each UI change | The owner sees the screens and picks one option |
| Product | `PRODUCT.md` | Who the users are, what the product is for, the design principles |
| Design system | `docs/DESIGN.md` | Colors, type, spacing and shared user interface (UI) parts |
| Roadmap | `docs/ROADMAP.md` | The order of the large pieces of work: Now, Next, Later |
| Specs | `docs/specs/<YYYY-MM-DD>-<name>.md` | The design of large or risky work |
| Design briefs | `docs/design/<YYYY-MM-DD>-<name>-brief.md` | The request to a designer (Claude Design or a person) |
| Decisions | `docs/decisions.md` | Each large decision, with the date and the reason |
| Release notes | The PR and the GitHub Release | What changed for users |

Do not keep a to-do list or a status table in a file, and do not use GitHub
Issues or another tool as a tracker. Use the board.

## The board

The board is `docs/board/board.html`, published as a claude.ai artifact with
the capabilities `{"db": {}, "user": {}}`. The owner drags cards and adds cards.
Agents add and move cards with the `ArtifactData` tool.

Each card is one document in the collection `cards`. The document ID is
`C-<number>`. The fields:

| Field | Value |
|---|---|
| `title` | The work, in a few words |
| `type` | `feature`, `bug`, `design` or `chore` |
| `priority` | `P1` (now), `P2` (next) or `P3` (later) |
| `status` | The column: `backlog`, `design`, `ready`, `building`, `review` or `done` |
| `needsYou` | What the owner must do, in one sentence. An empty string when nothing. |
| `doneWhen` | The result that the owner can see. One line for each check. |
| `links` | An object with `spec`, `compare`, `thread` and `pr`. Each value is an `https://` URL or is missing. |
| `order` | The card number. It sorts cards of the same priority. |
| `updatedAt` | The time of the change, in milliseconds since 1970 |

Rules for agents:

1. Before you add a card, list the collection `cards`. Use the next free number.
2. To change a card, use `update` with only the fields that change. Always set
   `updatedAt`.
3. Change only the card of your own work.
4. When you need the owner, set `needsYou`. When the owner answers, set it to an
   empty string.
5. If `board` in `starter.json` is empty, publish `docs/board/board.html` as a
   new artifact with the capabilities above. Put the URL in `starter.json` in
   the PR of your change.

Only an agent with the `ArtifactData` tool (a Claude session) can change the
board. Codex reviews the code and does not change the board.

## How work moves

| Column | The card is here when | The next step |
|---|---|---|
| Backlog | Someone asked for it | Give it a priority and a `doneWhen` |
| Design | It needs a spec, a design brief or a choice from the owner | The owner agrees the spec or picks an option |
| Ready | It has `doneWhen` and a test plan, and each choice is made | A thread starts it |
| Building | A thread works on it | The thread opens a PR |
| Review | The PR is open and in the review loop | The review is clean and the PR merges |
| Done | The PR merged into `develop` | The release PR takes it to `main` |

Put the card ID in the PR summary (`Card: C-<number>`), and put the PR URL in
`links.pr`. When the PR merges, move the card to `done`.

## When to write a spec

Write a spec for work that is one of these:

- Architectural: it changes how parts of the product connect.
- Ambiguous: two people can read the request in different ways.
- Irreversible: it cannot be undone after the release.
- Risky: external writes, sign-in, personal data, payments, migrations, deletes.

Copy `docs/templates/spec.md`. A copy edit, a one-line fix or a format change
needs no spec. Give each fact that you did not check in the `## Assumptions`
section.

## UI changes: before and after

For each change to a screen, the owner picks the result from screenshots before
the merge. Do these steps:

1. Read `PRODUCT.md` and `docs/DESIGN.md`.
2. Take screenshots of the screen as it is on `develop`: one at phone size
   (390 by 844) and one at desktop size (1440 by 900). Use Playwright, or the
   test tools of the product.
3. Build the new version. If the design is open, build two or three options.
   Take the same screenshots of each option.
4. Copy `docs/board/compare.html` to a scratch folder. Fill in its data block:
   the title, the card ID, the question, the recommended option, the reason,
   and the image path of each option at each size.
5. Publish the page as a new artifact with the capabilities
   `{"db": {}, "user": {}}`. Publish the screenshots in `files`, at the paths of
   the data block.
6. Put the URL in `links.compare` of the card. Move the card to `design` and set
   `needsYou` to "Pick an option".
7. Tell the owner. In a project thread, post the link and a decision card with
   the same options, so that the answer wakes the thread.
8. Read the answer: `ArtifactData`, collection `choice`, document `answer`. It
   has `option`, `label` and `note`. If the decision card and the page do not
   agree, use the newer answer and say so.
9. Build the chosen option. Do the changes of the note. Delete the code of the
   other options. Clear `needsYou`.
10. In the PR, link the artifact and name the chosen option in `## Summary`.

A change that the owner cannot see on a screen (a refactor, a test, a style
change with no visible effect) needs no before/after. Say so in the PR.

## Design work

1. Read `PRODUCT.md` and `docs/DESIGN.md`.
2. For a new screen or a new flow, write a design brief from
   `docs/templates/design-brief.md`. Say which parts are fixed and which parts
   the designer decides.
3. Put the handoff files in `docs/design/<YYYY-MM-DD>-<name>/`, with a
   `README.md`.
4. Build the screens with the tokens and the parts of `docs/DESIGN.md`. If the
   design adds a token or a part, add it to `docs/DESIGN.md` in the same PR.
5. Show the result with a before/after page, as above.

## Work with many parts

1. A planning thread writes the spec, then adds one card for each part. Each
   card has a `doneWhen` and says which files it changes.
2. Do not build two cards that change the same files at the same time.
3. Each thread works on one card and opens one PR.
4. When each card is in `done`, the planning thread checks the result against
   the spec and reports once.

## Review the plan

Once each week, or when a large piece of work ends:

1. Move each card that is no longer wanted to `done`, with the reason in
   `doneWhen`.
2. Check the priorities of the open cards.
3. Update `docs/ROADMAP.md`.
