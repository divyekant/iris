# Spec: open model version 1 and the conformance kit

- Date: 2026-11-17
- Card: R6-07
- Status: Draft

## Problem

The open model is the set of JSON documents that Iris writes and reads: the
five forms, the sender, the matter, the changeset, the policy file, the job,
the job item and the person. Version 0 is a draft. `docs/spec/README.md`
says: "Each field can change before version 1. Do not build a product that
needs a stable format on version 0."

So no project outside Iris can build on the model now. An agent tool, a
second mail client or a converter has no stable format, no rule for a later
change, and no test that tells it that its documents are correct.

## Goal

1. Version 1 of the open model is frozen. Each schema in
   `docs/spec/schema/` has the version 1 marks, and Iris writes only
   version 1 documents.
2. A published text gives the rules of change inside version 1, and the
   rules for a producer and a reader of the model.
3. A conformance kit checks the documents of a producer, with no Iris code:
   the schemas, the sample documents (fixtures) and a small runner.
4. A sample producer in another language (Python, standard library only)
   writes documents that pass the kit. This proves the card's
   "done when": a project outside Iris passes the tests.

## Not in scope

- New documents. These documents stay outside version 1.0. A later minor
  version (section 2) can add each one, because a new schema is an
  addition:
  - the message document of `iris show m-N --json` and of `messages.jsonl`
    in `iris export`;
  - the ledger entry (spec 2, section 7.1), the watch and the notice;
  - the draft content file of `iris draft <thread> <file>`.
- New verbs. `nudge`, `wait` and `ask` are not built. A minor version adds
  each verb when its card is built (section 2, rule 3).
- A rule check that JSON Schema cannot express (issue #174, for example a
  comparison of two fields). The kit checks the schemas only.
- A reader conformance test that runs a reader program. Section 4 gives the
  reader rules, and the self-test checks the validator of a reader.
- A runner in a second language. The schemas and the fixtures are the kit.
  A project can run them with its own JSON Schema validator.

## Design

### 1. What version 1 is

Version 1.0 is the model of `develop` on the day that the freeze PR merges,
with only these changes:

| Change | Where |
|---|---|
| `"model": "iris/1"` in place of `"iris/0"` in each document | each schema with a `model` field, each sample, each writer in the code |
| `$id` `urn:iris:model:1:<name>` in place of `urn:iris:model:0:<name>`, also in each `$ref` | each schema |
| The title `Iris open model 1: <name>`, with no `(draft)` | each schema |
| The description of `attention` names spec 2, section 6.4, in place of "Reserved for specification 2" | `common.schema.json` |
| The policy `version` accepts `0` and `1`, with the same meaning | `policy.schema.json`, `iris-policy` |
| Each `date` value (a day with no time) gets the `pattern` `^[0-9]{4}-[0-9]{2}-[0-9]{2}$` beside `"format": "date"` (section 5.4) | `common.schema.json` `$defs/date`, the `defer` date of `changeset.schema.json`, the rule `added` of `policy.schema.json` |
| Each section of the spec text that says "version 0" says "version 1" | `docs/spec/01-mirror-store-forms.md`, `docs/spec/02-changeset-policy-ledger.md`, `docs/spec/03-job-workspace.md`, `docs/spec/README.md` |

No field, enum value or rule of a schema changes in other ways. The date
`pattern` adds no new limit for a validator that checks `format`. The freeze
is a version mark, not a redesign. The fields of version 0 are in use in
the code and in the web client. A redesign at the freeze makes every
reader change and gives no gain that a later minor version cannot give.

The policy file keeps `version = 0` valid because the policy file of the
owner exists on disk. Iris writes the file only for a change from the web
client, and it keeps each other line (spec 2, section 6.3). A version 0
file that became invalid would put Iris in observe mode (fail closed). The
first-start file and the samples write `version = 1`.

### 2. Rules of change inside version 1

These rules go into a new published text,
`docs/spec/04-open-model-versions.md` (specification 4).

1. The `model` field stays `"iris/1"` for each minor version of
   version 1. The kit version (`docs/spec/VERSION`, for example `1.0`)
   names the minor version.
2. A minor version can only add:
   - an optional field;
   - a new value of an enum;
   - a new verb, with its arguments;
   - a new schema (a new document).
3. A minor version never removes or renames a field or an enum value. It
   never changes the type or the meaning of a field. It never makes an
   optional field required, and it never makes a rule stricter.
4. Each valid sample of an older minor version stays in `examples/` and
   stays valid. The self-test (section 5.2) checks this rule.
5. Each other change is a new major version: `"iris/2"` and
   `urn:iris:model:2:<name>`.
6. The maintainer approves each change of the model (as now,
   `docs/spec/README.md`).

### 3. Rules for a producer

A producer writes documents of the model. Iris is one producer.

- Write only the fields of the minor version that the producer claims.
- Write the `model` value `"iris/1"`.
- Write each document so that it passes the schema of its kind, with no
  `format` check (section 5.4).
- Do not write a key two times in one JSON object.
- Do not write `NaN`, `Infinity` or `-Infinity`.

A producer conforms when each document that it writes passes
`run.py check` (section 5.2).

### 4. Rules for a reader

A reader reads documents of the model. Iris is a reader of the policy file.

- Accept each document of each minor version of version 1.
- If a reader validates its input, use the schemas of the newest kit
  version of version 1. Rule 2.4 makes these schemas accept each document
  of an older minor version.
- If a reader does not validate its input, ignore an unknown field.
- If a reader finds an unknown enum value, it treats the one object that
  holds the value as not known. It can show the object as not known, or
  skip it.

  CAUTION: A reader must never take an action on an object with an unknown
  verb or an unknown state. A minor version can add a verb, and a reader
  of an older version does not know its effect.

- Refuse a document with a `model` value other than `"iris/1"`.

### 5. The conformance kit

#### 5.1 Files

| Path | Content | New |
|---|---|---|
| `docs/spec/schema/*.schema.json` | The schemas of version 1 | no (changed) |
| `docs/spec/schema/examples/` | The valid fixtures: each file must pass | no (changed) |
| `docs/spec/schema/examples/invalid/` | The invalid fixtures: each file must fail | no (changed) |
| `docs/spec/VERSION` | The kit version, `1.0` | yes |
| `docs/spec/04-open-model-versions.md` | Specification 4: sections 2, 3, 4 and 5 of this design, as the published text | yes |
| `docs/spec/conformance/README.md` | How to run the kit, and the result of each exit code | yes |
| `docs/spec/conformance/run.py` | The runner | yes |
| `docs/spec/conformance/requirements.txt` | The dependencies of the runner, with exact versions and hashes | yes |
| `docs/spec/conformance/producer/produce.py` | The sample producer | yes |
| `docs/spec/conformance/producer/mail/*.eml` | The input mail of the sample producer: synthetic messages only | yes |

The fixtures stay in `docs/spec/schema/examples/`. A second copy in
`conformance/` would be two homes for one fact. The name rule of the
fixtures does not change: the name starts with the schema name, for example
`commitment-payment.json` uses `commitment.schema.json`.

#### 5.2 The runner

`run.py` needs Python 3.11 or newer (for `tomllib`) and one library:
`jsonschema` (with its dependencies), at exact versions with hashes in
`requirements.txt`. Install with `pip install --require-hashes -r
requirements.txt`.

| Command | Does |
|---|---|
| `python3 run.py self-test` | Validates each file of `examples/` (each must pass) and of `examples/invalid/` (each must fail). Prints one line for each file that does not give the expected result. |
| `python3 run.py check <folder>` | Validates each `.json`, `.jsonl` and `.toml` file at the top level of the folder. A `.jsonl` file holds one document on each line. Prints one line for each file or line that fails. |
| `python3 run.py --version` | Prints the kit version from `docs/spec/VERSION`. |

The runner selects the schema by the file name: the text before the first
`-` or `.`. A `.toml` file is a policy file. The runner converts TOML to
JSON with the same verdict as `crates/iris-spec/tests/policy.rs`: a TOML
date or time becomes its RFC 3339 text (section "Assumptions").

The runner loads each schema of `docs/spec/schema/` into one registry under
its `$id`. It never gets a schema from the network.

Exit codes:

| Code | Meaning |
|---|---|
| 0 | Each file gave the expected result, and the folder had one or more documents. |
| 1 | One or more files failed, or the folder had no document. |
| 2 | The command is not correct (usage), or the kit is not complete (a schema does not load). |

#### 5.3 The sample producer

`producer/produce.py <out-folder>` uses only the Python standard library.
It reads each `.eml` file in `producer/mail/` with the `email` package, and
writes one document for each message:

- A message with a `List-Id` header gives a `feed` item
  (`feed-<n>.json`).
- A message with a one-time code (a fixed rule: a line with six digits
  after the word "code") gives a `signal` item (`signal-<n>.json`).
- Each other message gives nothing.

The producer gives its own IDs (`f-1`, `s-1`, `m-1`, `t-1`). The rules are
on purpose small. The producer proves that a program with no Iris code can
write documents that pass the kit. It does not copy the understanding step
of Iris.

#### 5.4 The `format` keyword

Validators do not agree on `format`. In JSON Schema draft 2020-12, `format`
is an annotation unless a validator turns on format checks. The Rust crate
`jsonschema` and the Python library `jsonschema` check `email` with
different rules.

So the kit does not depend on `format`:

- Each invalid fixture fails also with format checks off. The Rust test
  checks this rule (section "Acceptance criteria", item 3).
- Two invalid fixtures fail only by `format` now:
  `changeset-defer-bad-date.json` and `policy-added-time.toml`. The date
  `pattern` of section 1 makes both fail with format checks off.
- The runner runs with format checks off.

The Iris tests keep format checks on (`iris_spec::validator`), so Iris as a
producer meets the stricter rule.

### 6. Changes in Iris

1. One constant for the model value, `iris_store::MODEL = "iris/1"`. Each
   writer uses it: `iris-understand`, `iris-talk`, `iris-observe`,
   `iris-changeset`, `iris-job`, `iris-workspace` and `iris-api`
   (`matters.rs`). Each of these crates depends on `iris-store` now.
   `crates/iris-understand/src/schema.rs` validates each item in the
   program against the embedded form schemas. Its registry key
   `urn:iris:model:0:common` becomes a constant beside `MODEL`, so the
   `$id` and the key cannot differ.
2. A store migration rewrites the `model` of each stored item:
   `UPDATE item SET doc = json_set(doc, '$.model', 'iris/1') WHERE
   json_extract(doc, '$.model') = 'iris/0'`. Only the item documents are
   stored. The changeset, the job and the matter documents are written
   when they are read.
3. `iris-policy` accepts `version = 0` and `version = 1`.
4. `crates/iris-spec`: the expected `$id` and title in
   `tests/schemas.rs`, and a new test `tests/conformance.rs` (section
   "Acceptance criteria").
5. `starter.json`: a new area `spec-kit` with the paths
   `docs/spec/schema/**`, `docs/spec/conformance/**` and
   `docs/spec/VERSION`. Its `setup` makes a virtual environment in
   `.local-ci/` and installs `requirements.txt` with hashes. Its `test`
   runs `run.py self-test`, then `produce.py` into a new temporary folder,
   then `run.py check` on that folder.

### 7. Order of the work

| PR | Content | Depends on |
|---|---|---|
| 1 | This spec | none |
| 2 | The freeze: section 1, section 6 items 1 to 4 (without `conformance.rs`), the spec text changes | PR 1 approved by dk |
| 3 | The kit: specification 4, `docs/spec/VERSION`, `conformance/`, `conformance.rs`, the `spec-kit` area | PR 2 |

PR 2 changes the schemas and the code together, because the tests of each
writer validate its output against the schemas.

## Options not chosen

- Keep version 0 and only add the kit. Not chosen: the card is the freeze,
  and a kit for a draft model gives no stable target.
- A redesign of fields at the freeze (for example one ID scheme or one date
  format). Not chosen: each reader changes, and section 2 lets a minor
  version add what is missing.
- Strict major versions only (each addition is a new major version). Not
  chosen: each new verb of releases 5 and 6 would break each reader.
- A minor version in the `model` value (`"iris/1.1"`). Not chosen: a reader
  that compares the value breaks at each addition. The kit version names
  the minor version.
- Validators checked for the runner:
  - Python `jsonschema` (maintained, draft 2020-12, offline registry
    through `referencing`): chosen. Python has `tomllib` in the standard
    library, so the runner needs no TOML library.
  - Node `ajv` with `ajv-formats`: not chosen. Node has no TOML parser in
    its standard library, so the runner needs a second dependency.
  - The Rust crate `jsonschema` of `iris-spec`: not chosen for the runner,
    because the kit must run with no Iris code and no Rust build. The Rust
    tests use it. Two validators that give the same verdict on each fixture
    are a second check of the kit.
- A runner with no library (an own JSON Schema validator in the standard
  library). Not chosen: AGENTS.md says to use a maintained library for a
  format checker.
- A separate copy of the fixtures in `conformance/`. Not chosen: one fact,
  one home.

## Failure list

The runner reads files that the product does not control.

Supported input: a folder with files at its top level, each UTF-8 JSON
(`.json`), JSON Lines (`.jsonl`) or TOML (`.toml`), with a name that starts
with a schema name of the kit.

| Input | Result | Test |
|---|---|---|
| A file that is not valid UTF-8, JSON or TOML | That file fails with the parse error. Exit 1. | `conformance.rs` runs `check` on a bad file |
| A JSON object with a key two times | That file fails ("duplicate key"). | same |
| `NaN`, `Infinity` or `-Infinity` | That file fails. | same |
| A name with no schema of the kit | That file fails ("no schema"). It is not skipped. | same |
| An empty `.jsonl` line | That line fails. | same |
| A folder with no `.json`, `.jsonl` or `.toml` file | Exit 1 ("no documents"). | same |
| A subfolder | Not read. | same |
| A TOML date or time | Its RFC 3339 text. The self-test proves that the Python and the Rust converters give the same verdict on each sample. | `self-test` on `policy-*.toml` |
| A schema with a `$ref` to an `$id` that is not in the kit | Exit 2 at load. No network call. | `conformance.rs` |
| A validator that checks `format` | Same verdict on each fixture (section 5.4). | `conformance.rs`, format off |
| A very large file | The runner reads it into memory. The runner is a test tool, not a part of the product. | none (issue) |
| Any other error | Exit 2 with the error. | none |

Crash recovery: the store migration of section 6 runs in the migration
transaction, after the backup copy before a migration (spec 1, section
5.5). A crash leaves the old version and the old documents.

A job workspace that a session uses during the upgrade keeps its `iris/0`
files until its next build. Spec 3, section 11 builds the workspace before
each session, so the next session gets `iris/1` files.

## Impact

- Data format: the `model` value and the `$id` of each schema. Each other
  field stays.
- Store: one migration of `item.doc`. The triggers `attention_due_item` and
  `attention_keep` run on each item one time. `attention_keep` keeps the
  attention. `attention_due_item` makes the attention thread compute the
  attention of each item again one time.
- Memory: no change in the `iris` program. The runner and the producer are
  not a part of it.
- API and screens: no change. The web client does not read `model`
  (`grep -rn 'iris/0' client/src` finds nothing).
- Issues: #175 closes in PR 2 (the schema has `draft`, `send` and `held`
  now; a minor version adds `nudge`, `wait` and `ask`). #174, #176, #185
  and #395 stay open: the schemas of version 1 keep these limits.

## Assumptions

- Only `item.doc` stores a document with a `model` value. Evidence:
  `grep -rn "iris/0" crates` finds only writers and tests, and
  `crates/iris-store/src/schema_1.sql` has the one `doc` column.
- Each writer of a `model` value depends on `iris-store`. Evidence:
  `grep '^iris-store' crates/<crate>/Cargo.toml` finds the line in each of
  the seven crates of section 6.
- The web client and the agent tools do not compare `model`. Evidence:
  `grep -rn 'iris/' client/src crates/iris-session/src` finds nothing.
- The Python `jsonschema` library and the Rust crate give the same verdict
  on each fixture of `develop` today. Evidence: a script in this session
  ran each file of `examples/` and `examples/invalid/` through Python
  `jsonschema` 4.23.0. With format checks on, each file gave its expected
  verdict, as in `cargo test -p iris-spec`. With format checks off, only
  `changeset-defer-bad-date.json` and `policy-added-time.toml` passed in
  place of a fail. Section 1 adds the date `pattern` for them. The
  `spec-kit` test keeps the check for each later fixture.
- PyPI is reachable from a cloud session. Evidence: `pip download
  jsonschema==4.23.0` succeeded in this session. If a session cannot
  reach it, the `spec-kit` setup fails and local CI says so.
- `tomllib` converts each TOML sample to a JSON value with the same
  verdict as the Rust `toml` crate, when a TOML date or time becomes the
  text of Python `isoformat()`. Evidence: the same script, on each
  `policy-*.toml` sample. The two texts can differ for one value (Python
  writes `+00:00`, Rust writes `Z`). No schema compares that text, so the
  verdict is the same. The self-test keeps the check.

## External writes

None. The kit, the producer and the migration write only local files.

## Open questions

- The exact versions of `jsonschema` and its dependencies. The build picks
  versions that are more than 2 days old and writes the hashes.
- The text of a "not known" object in a reader of Iris. No reader of Iris
  meets an unknown value in version 1.0.

## Acceptance criteria

1. Each schema has `$id` `urn:iris:model:1:<name>` and no `(draft)` in its
   title. Red if `tests/schemas.rs` finds a schema with a version 0 mark.
2. Each document that Iris writes has `"model": "iris/1"`. Red if a writer
   test validates an `iris/0` document against a version 1 schema.
3. Each invalid fixture fails with format checks off in the Rust
   validator. Red if a fixture fails only by `format`.
4. After the migration, no stored item has `"model": "iris/0"`. Red if the
   migration test finds one.
5. A policy file with `version = 0` and one with `version = 1` both load.
   Red if the policy test with a version 0 file puts Iris in observe mode.
6. `run.py self-test` exits 0 in the `spec-kit` area. Red if a fixture
   gives a different verdict in Python than in Rust.
7. `produce.py` writes one or more `feed` and `signal` documents, and
   `run.py check` on its folder exits 0. Red if the producer writes a
   field that a schema does not allow, or if it writes nothing (exit 1).
8. `run.py check` on a folder with a bad file exits 1 and names the file.
   Red if the runner skips the file.
