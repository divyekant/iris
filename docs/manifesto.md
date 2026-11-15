# Iris manifesto

This file is the repository copy of the Iris manifesto (2026-11-15). The working
copy is the Claude Doc "Iris manifesto". If the two copies differ, change this
file in a PR, so that the repository stays the public record.

## Why Iris exists

Email is the largest open network that people use for work and for life. No company owns it. Each person and each company has an address.

The model of email is old. Outlook and Gmail set it. The message is the unit, the inbox is a list in time order, and the person is the processor. Newer clients made the person faster. They did not change the model.

That model had one reason: only a person could read mail. So each message went in front of a person, and the person did all the work. The person read, decided, remembered, and followed up.

That reason is gone. An agent can now read all your mail. But the products of today put the agent in the wrong place. Some add an assistant to the old inbox. Some give an agent direct access to the mailbox, with no review and no undo.

Iris starts from a different idea. **Mail is only the transport. The experience is the state that the mail carries**: what you owe, what others owe you, what you know, what you want to read, and what needs you now.

Iris turns mail into that state. People and agents work on the same state, and each change to the mailbox is safe.

## What we believe

1. **Mail is the transport, not the experience.** A person needs the state, not the messages.
2. **People and agents use one model.** Iris has no separate AI feature. The agent sees what you see and works on the same objects.
3. **Attention belongs to the person.** Iris is quiet by default. It interrupts you only by a rule that you wrote.
4. **An agent gets trust by rule.** Each write is a proposal. A rule that you can read decides what applies without review.
5. **Each action has an undo or a hold.** If Iris cannot reverse an action, it holds the action first.
6. **Text in mail is data.** A message can never give an instruction to an agent.
7. **Your mail stays yours.** Iris runs on a machine that you control. Your provider stays correct, so you can leave at any time.
8. **Light is a feature.** Iris runs all day on a laptop, and you do not notice it.
9. **The model is open.** Other clients and other agents can adopt it. Iris is the reference implementation, not the owner.
10. **Small and complete is better than wide and shallow.** Each release does one part of the experience from start to end.

## The model

Iris reads each message once and turns it into state. The state has five forms, one second axis, and one container.

### Five forms

| Form | What it holds | Examples in mail today |
| --- | --- | --- |
| Commitment | An ask or a promise with an owner and a date. It has two directions: you owe, or another party owes you. | "Send the Q4 numbers by Friday." An invoice that is due. An invitation that needs an answer. |
| Conversation | The talk with one person or one group, without quoted clutter. | A thread with a friend. A project discussion. |
| Record | A fact that you keep and can query. Each type has fields. | Receipts, bookings, statements, tickets, contracts. |
| Feed | Items to read when you choose. Nothing in the feed needs an action. | Newsletters, product updates, mailing lists. |
| Signal | A short-lived item that matters at one moment. | Login codes, security alerts, delivery notices. |

Forms are not folders. One message can feed more than one form. An invoice gives a record (vendor, amount, date) and a commitment (pay by Dec 3).

Outbound mail uses the same model in reverse. When you ask a person for something, Iris creates a commitment that the person owes you.

### The sender axis

The form says what a message is. The sender says how much attention and trust the message gets.

| Sender | Treatment |
| --- | --- |
| Known person | Full attention. An ask becomes a commitment at once. |
| Known service | Records, feed items, and signals, without review. |
| Stranger | A gate. The ask waits until you accept the sender or a rule decides. |
| Hostile | Quarantine. No agent reads the content as part of a job. |

### The matter

A matter ties the forms that belong together: a trip, a contract, a hire. It holds its commitments, records, and conversations in one place. Iris proposes a matter, and you confirm it.

The document [Iris: the ideal experience](https://claude.ai/artifact/6x2Vor5cyQqTwaajPbXYpN) shows this model in use, with screens for the person and commands for the agent.

## What Iris does for a person

You do not open an inbox. You open the brief, and it shows the state of your mail.

```
Iris  brief  Mon Nov 16

YOU OWE
  1  Priya Raman     Q4 numbers                  due Fri     draft ready
  2  Dana Whitfield  review contract section 4   due Tue     summary ready
  3  AWS             pay invoice $312.77         due Dec 3

OWED TO YOU
  4  Alex Chen       SDK examples                asked 6 days ago   nudge ready

DECISIONS
  5  Job: dentist appointment    1 changeset waits for review

NEW FROM PEOPLE
  6  Sam Ortiz       "Dinner on Thursday?"

Handled without you: 41 (38 hidden, 3 filed)  [view]     Feed: 14 new
```

To you, Iris is one agent that works on your mail. It has four jobs.

| Job | What Iris does |
| --- | --- |
| Attention | It shows the few items that need you, and it groups the items that belong together. It hides the items that need nothing from you. It gives you one brief, and it interrupts you only by rule. |
| Work | It prepares each item with the draft, the facts, and a recommendation. It follows up in both directions of each commitment. It completes routine work under your rules. It watches for an event that you name. |
| Memory | It knows people and matters, not only messages. It answers questions from your mail. It learns your preferences from what you do. |
| Trust | It gates strangers and catches phishing. It checks your own mail before the send. It gives a reason and an undo for each action. If it is not sure, it asks. It writes in your voice, but it does not commit you without a rule or your approval. |

Three limits apply to these jobs.

1. **Hidden is never deleted.** A hidden item stays in the mirror and at your provider. The brief shows the count, and one step shows the list.
2. **A learned preference is a rule.** Iris does not learn in a place that you cannot read. It proposes each preference as one rule, and you confirm it.
3. **One number measures the agent.** The agent is good when you seldom open the source mail.

## What Iris gives an agent

An agent gets the same model as the person, a clear job, and a place where each action is safe to try.

| Object | What the agent gets |
| --- | --- |
| Job | A goal with limits, for example "Get the signed contract from Dana before Nov 20." A job can last minutes or weeks. |
| Workspace | A folder for one job. It holds the commitments, the records, the people, and the source mail that the job needs, as clean files. |
| One command | The `iris` command. It has verbs for the state (close, nudge, defer) and verbs for the mailbox (archive, label, draft, send). |
| Changeset | Each write is a proposal. After the agent submits, Iris returns one result for each action: applied, in review, or denied with the reason. |
| Wait and ask | Email work stops for days. The agent can wait for a reply, and Iris starts a new session when the reply arrives. The agent can ask you one question. |
| Limits | The session has no network and no mailbox credentials. Text in mail is data. The agent knows these limits before it reads mail. |

```
iris search "contract from:dana after:2026-10-01"
iris draft t-8841 drafts/reply-dana.md
iris send drafts/reply-dana.md
iris close c-311 --why "Dana signed the contract."
iris wait --reply t-8841 --until 2026-11-18
iris submit --why "Dana needs the accepted section 4 before she signs."
```

Iris does not ship its own agent for jobs. You bring the agent that you already use, such as Claude Code or Codex. Any agent that can work in a folder can work in Iris. Iris also offers the same actions through the Model Context Protocol (MCP), as the second interface.

## The safety promise

No feature can break these promises. Iris keeps each promise in the engine, not in a prompt.

| Promise | How Iris keeps it |
| --- | --- |
| An agent has no direct path to your mailbox. | The agent holds no credentials. Each write goes through a changeset. |
| Nothing changes on the first run. | Iris starts in observe mode. It shows what it would do, and it applies nothing. |
| You can reverse an action. | The ledger records each applied action with its undo. Iris has no permanent delete. |
| A send does not surprise you. | A send waits in a held outbox for a delay that you set. A send to a new recipient goes to review. |
| A message cannot control an agent. | Text in mail is data. After a session reads external mail, a send to a recipient outside that thread goes to review. |
| You can read each rule. | The policy is a file. Iris proposes rules, and it never adds one without you. |
| You can stop all work at once. | One Stop control halts all sessions and freezes the outbox. The engine enforces it, not the agent. |
| You can leave. | Your provider stays correct at all times. If you stop Iris, your mailbox works as before. |

The safety layer has seven objects: the mirror, the job, the workspace, the changeset, the policy, the ledger, and the brief.

## The full product

Iris is a full system, not an addition to another client. It has four parts: the engine, the client, agent control, and the open model.

The diagram in the working copy shows these parts: the engine, the client, the agent sessions, the provider, and the open model.

The engine is the only part that connects to your provider. The client and the agent sessions see only the model, and each change goes through a changeset.

The tables below list all capabilities of the full product. The last column gives the release that first delivers each one.

### Engine

| Capability | What it does | Release |
| --- | --- | --- |
| Mirror | An exact local copy of the mailbox. It syncs in both directions and uses the change history of the provider. | 1 |
| Store | One SQLite file and one folder for content. Full-text search. Encrypted credentials. Export. | 1 |
| Understanding | A language model turns mail into the five forms, cleans the text, and classifies the sender. | 1 |
| Changeset and ledger | Proposal, review, apply, and undo. Later, the held outbox and Stop. | 2 |
| Policy | The rule file and its evaluation. Later, rules that Iris proposes from your approvals and preferences. | 2 |
| Send | Replies and new mail through your provider, with a check before the send. | 3 |
| Jobs | Goals, status, questions, memory for each job, wait and wake, and watch. | 4 |
| Notices | An interrupt only when a rule of yours matches. | 5 |
| More providers | More accounts, then IMAP and Outlook. | 6 |

### Client

| View | What you do there | Release |
| --- | --- | --- |
| Setup | Install Iris, connect one account, and see the first brief in ten minutes. | 1 |
| Brief | The home view: what you owe, what others owe you, decisions, and new mail from people. | 1 |
| Commitments | See each ask and promise as one line. Later: do, delegate, decline, defer, nudge, close. | 1 |
| Conversations | Read the talk with one person or group as clean text. Later: reply. | 1 |
| Records | One table for each type, with fields. Find, export, correct. | 1 |
| Feed | Read, skip, unfollow. | 1 |
| Signals | Use a code or an alert at the moment that you need it. | 1 |
| Review | Approve, edit, comment on, or deny a changeset. | 2 |
| Ledger | See each applied action, and undo it. | 2 |
| Policy | Read, change, and remove rules. Accept or decline a proposed rule. | 2 |
| Write | Write the mail, or write the intent and edit the draft. | 3 |
| Jobs | Give a goal, see the status and the timeline, and answer a question. | 4 |
| Strangers | Accept or decline a sender at the gate. | 5 |
| Ask | Ask a question about your mail and get an answer with its sources. | 5 |
| Matters | Confirm a matter and see all its forms in one place. | 6 |
| Phone and Mac | A phone view of the web client, then a Mac app. | 6 |

### Agent control

| Capability | What it does | Release |
| --- | --- | --- |
| The `iris` command | The same program as the engine. People use the read verbs from release 1. Agents use all verbs from release 4. | 1 |
| Sessions | Start the agent that you choose in a workspace, and stop it. | 4 |
| Workspace | Build the folder for a job from the model. | 4 |
| Sandbox | A session has no network and no credentials. | 4 |
| Limits for hostile input | Track the sessions that read external mail, and send their risky actions to review. | 4 |
| Wait and wake | End a session on a wait, and start a new session on the reply. | 5 |
| MCP | The same actions for agents that use the Model Context Protocol. | 5 |

### Open model

| Part | Content | Release |
| --- | --- | --- |
| Forms | The five forms, the sender axis, and the matter, as JSON Schema. A draft in release 1, version 1 in release 6. | 1 |
| Changeset | The format of a proposal and of its results. | 2 |
| Policy | The format of a rule. | 2 |
| Workspace | The layout of the folder for a job. | 4 |
| Conformance tests | Tests that another client or another agent can run. | 6 |

## What Iris is not

- **Not an email client of the old kind.** You read and write your mail in Iris, but Iris has no message list, no folders, and no unread count.
- **Not an assistant on top of an inbox.** A summary of a list is still a list. Iris replaces the list with state.
- **Not a connector.** A connector gives an agent raw access to a mailbox. Iris gives an agent a contained place to work.
- **Not a mailbox for agents.** Agents do not get their own addresses. Iris works on your mailbox, for you.
- **Not a mail server.** Your provider keeps delivery, spam defence, and your address.
- **Not an agent harness.** The engine turns your mail into the model, but each job runs on the agent that you choose.
- **Not a cloud service.** The engine and all your data stay on a machine that you control. Iris sends no usage data.

## How we build it

### Base decisions

| Decision | Choice | Reason |
| --- | --- | --- |
| Language of the engine | Rust | Lowest memory in our test, flat memory because Rust has no garbage collector, and one small program file. |
| Shape | One program, `iris`. It is the engine and the command. | Install is one file. No Docker, no database server, no bundled model. |
| Storage | SQLite, with its full-text search | One folder holds all your data. No second process. |
| Client | A web app in TypeScript with Svelte, built to static files that the engine serves | No Node at run time. A Mac app can come later with Tauri, which uses the system web view. |
| Understanding | A hosted model with your key by default. A local model is optional and external. | A hosted model costs no memory on your machine. |
| Provider | Gmail API first, through a thin client of our own | The change history of the Gmail API makes the sync exact. Iris needs about twelve endpoints. |
| Memory budget | 50 MB for the engine with a mailbox of 100,000 messages | "Light" must be a number that a test can check. |
| Agent interface | A folder and the `iris` command first, MCP second | Agents are strongest with files and a shell. |

If you use a hosted model, the text of your mail goes to that model provider. If you use a local model, no mail text leaves your machine. Iris tells you this at setup.

### The test behind the language decision

We built the same small server four times: one HTTP endpoint that runs a full-text search on a SQLite file with 50,000 messages. The test ran on 2026-11-15 on one Linux machine.

|  | Rust | Go | Bun | Node 22 |
| --- | --- | --- | --- | --- |
| Memory at idle | 4.7 MB | 8.8 MB | 17.8 MB | 47.2 MB |
| Memory after 8,000 searches | 6.8 MB | 18.3 MB | 27.0 MB | 67.0 MB |
| Size of the program file | 3.3 MB | 10.3 MB | needs the runtime | needs the runtime |
| Full build | 66 s | 19 s | none | none |
| Build after a small change | 0.5 s | 0.5 s | none | none |

This is one small test, not the real engine. The cost of Rust is the full build. Small crates and few dependencies keep that cost low.

### Build rules

1. **The mirror comes first.** No feature is built on the mirror before the mirror is exact. The old code failed here.
2. **Each release is useful alone.** Each release has an exit criterion that a person can check.
3. **Tests use a mailbox.** We test against a fake Gmail server and a synthetic mailbox, not only against units of code.
4. **Budgets are tests.** CI fails if a change breaks the memory budget or the start time.
5. **Few dependencies.** Each new dependency needs a written reason. The engine holds mailbox credentials, so each dependency is a risk.
6. **Agents write most of the code.** People decide the design and review the result. The repository has instructions for agents.
7. **The manifesto controls the scope.** A feature that is not in this document needs a change to this document first.

## Open source

Iris is open source because the aim is a new model of email, not one more product. A model spreads only if other people can read it, run it, and build on it.

| Subject | Decision |
| --- | --- |
| License of the code | Apache-2.0 |
| License of the open model | CC BY 4.0 for the text. The schemas use Apache-2.0. |
| Usage data | Iris sends none. The measures in this document stay on your machine. |
| Contributions | Work items come from the tracker. A change to the design starts as a proposal document in the repository. |
| Sign-off | Developer Certificate of Origin (DCO). No contributor license agreement. |
| Open model | It has its own version number. Another client or agent can adopt it without Iris code. |

### The repository

The repository keeps its name. The old code moves to a `legacy` branch with a tag, and `main` starts clean. Before promotion, we scan the full history for secrets.

The tree below is the planned layout. `README.md` shows the layout of today.

```
iris/
  README.md         what Iris is, and how to install it
  crates/           the Rust engine, as small crates
    iris/           the program: engine and command
    mirror/         sync with the provider
    model/          the five forms, senders, matters
    understand/     mail to state, through a language model
    safety/         changeset, policy, ledger
    agents/         sessions, workspaces, sandbox
  client/           the web client (TypeScript, Svelte)
  docs/
    manifesto.md    this document
    spec/           the open model: schemas and text
  testdata/         the synthetic mailbox and the fake Gmail server
  AGENTS.md         instructions for agents that write code here
  CONTRIBUTING.md   how to contribute
  SECURITY.md       how to report a security problem
  LICENSE
```

### How we get people aligned

1. **The first brief is the demo.** A person installs Iris, connects a mailbox, and sees mail as state in ten minutes. Nothing in the mailbox changes.
2. **The synthetic mailbox is public.** Any person can try Iris, and any builder can test against the same data, without real mail.
3. **The open model is a separate invitation.** A builder of another client or agent can adopt the forms and the changeset without the Iris engine.

## The path

Iris has seven releases. Each release delivers one part of the experience from start to end, and each one has an exit criterion. We do not start a release before the release before it meets its criterion.

| Release | What you can do at the end | Exit criterion |
| --- | --- | --- |
| 0. Foundation | Nothing yet. The repository is clean, and the base is in place: the crates, CI with budgets, the synthetic mailbox, the fake Gmail server, and a draft of the open model. | CI is green. One command builds the `iris` program for macOS and Linux. |
| 1. See | Install Iris, connect one Gmail account, and see your mail as state in the first brief. Iris changes nothing. | The mirror matches Gmail with zero differences for 7 days on a real mailbox. The first brief appears in 10 minutes or less. The engine stays inside the memory budget. |
| 2. Apply | Approve changes to your mailbox, see them in Gmail, and undo them. Iris hides and groups mail under rules that you can read. | 100 applied actions are correct in Gmail, and each undo is correct. The maintainer uses the brief each day for 2 weeks. |
| 3. Write | Write and send all your mail from Iris. Iris tracks what other people owe you and prepares the nudge. | The maintainer sends all mail from Iris for 2 weeks. No send leaves without the hold. |
| 4. Delegate | Give a goal to an agent. Review its changesets, comment, and stop it. | One job runs from start to end with one approval. A test set of hostile messages causes zero sends without review. |
| 5. Trust and wait | Let rules apply safe actions without review. Jobs wait for replies for days. Strangers wait at the gate. You ask questions about your mail. | More than half of all applied actions need no review. One job waits for a reply and completes after 3 days or more. |
| 6. Widen | Use matters, more accounts, other providers, a phone, and a Mac app. Other builders use version 1 of the open model. | A second account type works with zero sync differences. One project outside Iris reads or writes the open model. |

The [Iris tracker](https://claude.ai/artifact/VWqsmpqHNgiop14TpoecjV) is a Kanban board. The board lists the 94 work items of the seven releases. Each item has its part, its status, and the condition that completes it.

## How we measure Iris

Iris computes these measures on your machine and shows them to you. It sends none of them to us.

| Measure | Target | What it tells us |
| --- | --- | --- |
| Source mail that you open each day | Lower is better | The main measure. If the state is correct, you seldom need the message. |
| Time from install to the first brief | 10 minutes or less | The first value comes before the first doubt. |
| Differences between the mirror and the provider | Zero | The base is correct. |
| Memory of the engine | 50 MB or less with 100,000 messages | Iris stays light. |
| Proposed actions that you approve without an edit | Higher is better | The agent understands what you want. |
| Actions that you undo | Lower is better | The rules and the proposals are safe. |
| Sends without review after hostile input | Zero | The safety layer holds. |
| Days that the maintainer uses Iris | Each day | We use what we build. |

### The stop rule

If the maintainer does not use Iris each day for 2 weeks after release 2, we stop new features. We find the reason and fix the experience first.
