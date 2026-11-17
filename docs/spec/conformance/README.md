# The conformance kit of the Iris open model

Use this kit to check that a program writes valid documents of the Iris
open model. The kit uses no Iris code. The rules are in
[specification 4](../04-open-model-versions.md).

## Install

You need Python 3.11 or newer.

1. Make a virtual environment:

   ```bash
   python3 -m venv .venv
   ```

2. Install the libraries with their hashes:

   ```bash
   .venv/bin/pip install --require-hashes -r requirements.txt
   ```

## Check the documents of a producer

1. Write the documents of your program into one folder. Give each file a
   name that starts with the name of its schema, for example
   `commitment-1.json` or `signal-7.json`. A `.jsonl` file holds one
   document on each line. A file with the name `items.jsonl` can hold items
   of each form.
2. Run the check:

   ```bash
   .venv/bin/python run.py check <folder>
   ```

The runner reads only the files at the top level of the folder. It prints
one line for each document that fails.

| Exit code | Meaning |
|---|---|
| 0 | Each document passed, and the folder had one or more documents. |
| 1 | One or more documents failed, or the folder had no document. |
| 2 | The command is not correct, or the kit is not complete. |

A document with a minor version higher than the kit fails ("newer than the
kit"). Get the newest kit to check it.

## Check your validator

If you use your own JSON Schema validator, run it on the fixtures in
`../schema/examples/` (each must pass) and `../schema/examples/invalid/`
(each must fail). Turn on the formats `date` and `date-time`. The runner
does the same check:

```bash
.venv/bin/python run.py self-test
```

## The sample producer

`producer/produce.py` is a small producer with no Iris code. It reads the
synthetic mail in `producer/mail/` and writes a feed item and a signal:

```bash
python3 producer/produce.py /tmp/out
.venv/bin/python run.py check /tmp/out
```

It refuses a folder that exists. It writes into `<folder>.partial`, and
renames it to `<folder>` after the last file.

## Tests

`test_run.py` tests the runner and the sample producer:

```bash
.venv/bin/python -m unittest test_run
```
