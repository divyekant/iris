# Iris specifications and the open model

This folder holds the specifications of Iris and the open model.

| File | Content | Release |
|---|---|---|
| [01-mirror-store-forms.md](01-mirror-store-forms.md) | The mirror, the store, and the data format of the five forms and the sender axis | 1 |
| [02-changeset-policy-ledger.md](02-changeset-policy-ledger.md) | The changeset, the policy file, the ledger and the `iris` command | 2 |
| [schema/](schema/) | Version 0 of the open model as JSON Schema (draft 2020-12), with sample documents | 1 |

## Status

Version 0 of the open model is a draft. Each field can change before
version 1. Version 1 comes in release 6. Do not build a product that needs a
stable format on version 0.

## Licenses

- The text of the specifications: Creative Commons Attribution 4.0 (CC BY 4.0).
- The schemas and the sample documents in `schema/`: Apache-2.0.

## Change a specification

1. Open a pull request that changes the specification.
2. Change the schemas and the samples in the same pull request.
3. Run the schema test: `cargo test -p iris-spec`.

The maintainer approves each change to a specification.
