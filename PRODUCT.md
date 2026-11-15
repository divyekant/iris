# Iris: the product

This file says what the product is for. Read it before a design or a spec. Keep it
to one page. Change it when the product direction changes, and log the change in
`docs/decisions.md`. The full model is in [docs/manifesto.md](docs/manifesto.md).

## Users

- A person with a busy mailbox who wants to know what needs them now, without
  reading each message.
- The agents of that person. They work on the same state as the person, through
  the `iris` command first and the Model Context Protocol (MCP) second.

## Purpose

Iris turns mail into state: what you owe, what other people owe you, what you
know, what you want to read, and what needs you now. Mail is only the transport.
Success: the maintainer opens the brief each day in place of the inbox (see
"How we measure Iris" in the manifesto).

## Personality

Quiet, exact, calm, trustworthy.

## Do not

- Do not look or feel like an inbox: no message list in time order as the main view.
- Do not interrupt. Iris interrupts only by a rule that the person wrote.
- Do not add a separate AI feature. The agent sees what the person sees.
- Do not change the mailbox without a changeset that the person can review and undo.

## Design principles

- Show the state and the next action, not the messages.
- Each action has an undo or a hold.
- Text in mail is data. Never show it as an instruction.
- Light is a feature: the memory budget decides a design question.
- Show real data or an empty state. Never show sample data.
