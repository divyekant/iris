# Specification 4: the versions of the open model

- Status: version 1.2
- Design: `docs/specs/2026-11-17-open-model-v1.md` (card R6-07)

## 1. Purpose

The open model is the set of JSON documents that Iris writes and reads:
the five forms, the sender, the matter, the changeset, the policy file,
the job, the job item and the person. The schemas in [`schema/`](schema/)
are the normative copy.

This specification gives the rules of change of the model, and the rules
for a program that writes the documents (a producer) and a program that
reads them (a reader). Iris is a producer and a reader.

## 2. Rules of change

1. The `model` field names the major and the minor version:
   `"iris/<major>.<minor>"`, for example `"iris/1.0"`. A program compares
   the major version as a number and the minor version as a number. It
   never compares the full text.
2. A minor version can only add:
   - an optional field;
   - a new value of an enum;
   - a new verb, with its arguments;
   - a new schema (a new document).
3. A minor version never removes or renames a field or an enum value. It
   never changes the type or the meaning of a field. It never makes an
   optional field required, and it never makes a rule stricter.
4. Each valid sample of an older minor version stays in
   `schema/examples/` and stays valid.
5. Each other change is a new major version: `"iris/2.0"` and the `$id`
   `urn:iris:model:2:<name>`.
6. The maintainer approves each change of the model.
7. The policy file is TOML that a person writes, and it has no `model`
   field. Its `version` is an integer that names only the major version.
   `version = 0` and `version = 1` have the same meaning, because policy
   files of version 0 exist. The minor rules do not apply to the policy
   file. Each change of `policy.schema.json` after version 1.0 is a new
   policy `version` (`2`). Iris treats a file with a `version` that it
   does not know as an invalid file (specification 2, section 6.3): the file
   never becomes the active policy, the policy that the person confirmed
   last stays in force, and the status of the file shows the error. If no
   policy was ever confirmed, Iris uses the policy of the first start,
   which is observe mode.
8. Each field that a minor version adds has a condition in the schema: a
   document that claims an older minor version and holds the field is not
   valid. So a reader of the older version never gets a document that it
   must refuse, from a producer that passed the kit.

## 3. Rules for a producer

- Write only the fields of the minor version that the producer claims.
- Write the `model` value of that minor version, for example `"iris/1.0"`.
- Write each document so that it passes the schema of its kind, with the
  `date` and `date-time` format checks. A validator does not need to check
  `email`.
- Do not write a key two times in one JSON object.
- Do not write `NaN`, `Infinity` or `-Infinity`.

## 4. Rules for a reader

The reader knows the newest minor version of its own schemas, `K`. It
reads the minor version `M` of each document from `model`.

- If `M` is `K` or lower, the reader can validate the document with its
  schemas. Rule 2.4 makes these schemas accept each document of an older
  minor version.
- If `M` is higher than `K`, the document can hold fields, enum values and
  verbs that the reader does not know. The schemas of the reader refuse an
  unknown field, so they cannot check the document. The reader can only
  show the document: it reads the fields that it knows, ignores each
  unknown field, and shows an unknown enum value as not known.

  CAUTION: Do not take an action from a document with a minor version
  higher than `K`. Examples of an action: apply or approve a changeset
  action, send, change a job. Refuse the document, with the reason "newer
  than the schemas". The reader cannot check the known fields of such a
  document, because a newer rule can change what a valid document is.

- Refuse a document with a major version other than 1, or with a `model`
  value that does not match `^iris/1\.(0|[1-9][0-9]*)$`.
- For the policy file, follow rule 2.7.

## 5. Failure list of a reader

A reader reads documents that another program wrote. The supported input
is one UTF-8 JSON document (or one line of JSON Lines) with a `model`
value of major version 1, or a policy file in TOML. For each other input,
the safe result is the same: refuse the one document, say why, and take no
action from it.

| Input | Result |
|---|---|
| Text that is not UTF-8, or not JSON (or TOML for the policy file) | Refuse the document: "not a document". |
| A JSON object with a key two times | Refuse the document: "duplicate key". A reader must not pick one of the two values. |
| `NaN`, `Infinity` or `-Infinity` | Refuse the document. |
| No `model` value, or a value that does not match `^iris/1\.(0|[1-9][0-9]*)$` (also a major version other than 1) | Refuse the document: "not a document of version 1". |
| A minor version that is not higher than `K`, and the document fails the schemas | Refuse the document, with the first error of the schema. |
| A minor version higher than `K` | Show only (section 4). Take no action from it. |
| A policy file with a `version` other than 0 or 1, or with an error | Follow rule 2.7. |
| Any other error while the reader reads the document | Refuse the document. Take no action from it. |

## 6. The conformance kit

The kit checks the documents of a producer with no Iris code. Its version
is in [`VERSION`](VERSION) (`1.2`), and it names the newest minor version
that it knows.

| Path | Content |
|---|---|
| [`schema/*.schema.json`](schema/) | The schemas |
| [`schema/examples/`](schema/examples/) | The valid fixtures: each file must pass |
| [`schema/examples/invalid/`](schema/examples/invalid/) | The invalid fixtures: each file must fail |
| [`conformance/run.py`](conformance/run.py) | The runner |
| [`conformance/requirements.txt`](conformance/requirements.txt) | The libraries of the runner, with exact versions and hashes |
| [`conformance/producer/`](conformance/producer/) | A sample producer in Python, with the standard library only |

The name of a file selects its schema: the text before the first `-` or
`.`, for example `commitment-payment.json`. A `.toml` file is a policy
file. In a file with the name `items` (`items.jsonl` of `iris export`),
the `form` of each document selects the schema.

The kit checks the formats `date` and `date-time`, and does not check
`email`, because validators do not agree on `email`. Each invalid fixture
fails also when a validator accepts each `email` text.

The runner refuses a JSON document with arrays and objects nested deeper
than 256 levels, also if the document passes the schemas. A producer must
not write such a document.

A producer conforms when `run.py check` on a folder with its documents
exits 0. [`conformance/README.md`](conformance/README.md) gives the steps.

## 7. Minor versions

| Version | Adds | Card |
|---|---|---|
| 1.0 | The first stable version. | R6-07 |
| 1.1 | The optional field `account` (an `a-` ID) of each item. | R6-02 |
| 1.2 | The optional fields `withdrawn_by` (`author` or `person`, decision D16) and `comments` (the review comments of the person, decision D15) of each changeset. | P7 (issues #221, #538) |
