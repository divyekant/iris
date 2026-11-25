# Iris specifications and the open model

This folder holds the specifications of Iris and the open model.

| File | Content | Release |
|---|---|---|
| [01-mirror-store-forms.md](01-mirror-store-forms.md) | The mirror, the store, and the data format of the five forms and the sender axis | 1 |
| [02-changeset-policy-ledger.md](02-changeset-policy-ledger.md) | The changeset, the policy file, the ledger and the `iris` command | 2 |
| [03-job-workspace.md](03-job-workspace.md) | The layout of the job workspace folder and the format of each file | 4 |
| [04-open-model-versions.md](04-open-model-versions.md) | The versions of the open model, the rules for a producer and a reader, and the conformance kit | 6 |
| [conformance/](conformance/) | The conformance kit: the runner, its tests and a sample producer in Python | 6 |
| [schema/](schema/) | Version 1 of the open model as JSON Schema (draft 2020-12), with sample documents | 1 |
| [schema/changeset.schema.json](schema/changeset.schema.json) | The changeset document of specification 2, section 5.1, as JSON Schema, with sample documents | 2 |
| [schema/policy.schema.json](schema/policy.schema.json) | The policy file of specification 2, section 6.2, as JSON Schema for its TOML data, with sample files | 2 |
| [schema/job.schema.json](schema/job.schema.json) | The job document of the job store spec (`docs/specs/2026-11-16-job-store.md`), as JSON Schema, with sample documents | 4 |
| [schema/jobitem.schema.json](schema/jobitem.schema.json) | The job item of specification 3, section 5, as JSON Schema, with sample documents | 4 |
| [schema/person.schema.json](schema/person.schema.json) | The person file of specification 3, section 7, as JSON Schema, with sample documents | 4 |

## Status

Version 1.3 of the open model is stable. Each JSON document that Iris
writes has the `model` value `iris/1.3`. Version 1.1 adds the optional
field `account` to each item. Version 1.2 adds signal members, the
authors `agent` and `person`, and additions to the matter, and the optional
fields `withdrawn_by` and `comments` to each changeset. Version 1.3 adds
the `drafts` target of a `draft` action (a change of a draft). Each document
of version 1.0, 1.1 and 1.2 stays valid. The policy file (TOML) has no `model` value: its
integer `version` is `1` (`0` has the same meaning). A minor version of version 1 only adds to the model, and
a change that is not an addition is version 2. Specification 4 gives the
rules: [04-open-model-versions.md](04-open-model-versions.md).

## Licenses

- The text of the specifications: Creative Commons Attribution 4.0 (CC BY 4.0).
- The schemas and the sample documents in `schema/`: Apache-2.0.

## Change a specification

1. Open a pull request that changes the specification.
2. Change the schemas and the samples in the same pull request.
3. Run the schema test: `cargo test -p iris-spec`.

The maintainer approves each change to a specification.
