#!/usr/bin/env python3
"""The runner of the Iris open model conformance kit.

See docs/spec/04-open-model-versions.md, section 5, and README.md beside
this file. It needs Python 3.11 or newer and the libraries of
requirements.txt. It uses no Iris code.

Exit codes: 0 each file gave the expected result; 1 a file failed, or the
folder had no document; 2 usage, or the kit is not complete.
"""

import datetime
import json
import math
import re
import sys
import tomllib
from pathlib import Path

SPEC = Path(__file__).resolve().parent.parent
SCHEMAS = SPEC / "schema"
FORMS = ("commitment", "conversation", "record", "feed", "signal")
MODEL = re.compile(r"^iris/1\.(0|[1-9][0-9]*)$")
SUFFIXES = (".json", ".jsonl", ".toml")


class KitError(Exception):
    """The kit cannot run: exit 2."""


def kit_version():
    text = (SPEC / "VERSION").read_text(encoding="utf-8").strip()
    major, minor = text.split(".")
    return text, int(major), int(minor)


def load_validators():
    try:
        from jsonschema import Draft202012Validator, FormatChecker
        from referencing import Registry, Resource
    except ImportError as e:
        raise KitError(f"a library is missing ({e}); run pip install --require-hashes -r requirements.txt")
    try:
        # Without rfc3339-validator, jsonschema skips the date-time check.
        checker = FormatChecker(["date", "date-time"])
    except KeyError:
        raise KitError("the date-time check is not active; install rfc3339-validator")
    schemas = {}
    registry = Registry()
    for path in sorted(SCHEMAS.glob("*.schema.json")):
        schema = json.loads(path.read_text(encoding="utf-8"))
        schemas[path.name.split(".")[0]] = schema
        registry = registry.with_resource(schema["$id"], Resource.from_contents(schema))
    ids = {s["$id"] for s in schemas.values()}
    validators = {}
    for name, schema in schemas.items():
        # A $ref to an $id that is not in the kit would fail only on the
        # document that reaches it, so check each one now.
        for ref in refs(schema):
            if not ref.startswith("#") and ref.split("#")[0] not in ids:
                raise KitError(f"{name}.schema.json: $ref {ref} is not in the kit")
        try:
            Draft202012Validator.check_schema(schema)
        except Exception as e:
            raise KitError(f"{name}.schema.json is not a valid schema: {e}")
        validators[name] = Draft202012Validator(schema, registry=registry, format_checker=checker)
    return validators


def refs(node):
    if isinstance(node, dict):
        for key, value in node.items():
            if key == "$ref" and isinstance(value, str):
                yield value
            else:
                yield from refs(value)
    elif isinstance(node, list):
        for value in node:
            yield from refs(value)


def no_duplicates(pairs):
    seen = {}
    for key, value in pairs:
        if key in seen:
            raise ValueError(f"duplicate key {key!r}")
        seen[key] = value
    return seen


def no_constant(name):
    raise ValueError(f"{name} is not JSON")


def parse_json(text):
    return json.loads(text, object_pairs_hook=no_duplicates, parse_constant=no_constant)


def toml_value(value):
    """TOML data to its JSON form: a date or a time becomes RFC 3339 text."""
    if isinstance(value, dict):
        return {k: toml_value(v) for k, v in value.items()}
    if isinstance(value, list):
        return [toml_value(v) for v in value]
    if isinstance(value, (datetime.datetime, datetime.date, datetime.time)):
        return value.isoformat()
    if isinstance(value, float) and not math.isfinite(value):
        raise ValueError("nan and inf are not JSON")
    return value


def select(base, doc):
    """The schema name of one document, or None (no form of the five)."""
    if base != "items":
        return base
    form = doc.get("form") if isinstance(doc, dict) else None
    return form if form in FORMS else None


def documents(path):
    """Each (label, schema name, document, parse error) of one file. A
    `.jsonl` line that does not parse is one entry with its error, so the
    other lines are still checked. An OSError goes to the caller."""
    data = path.read_bytes()
    base = re.split(r"[-.]", path.name, maxsplit=1)[0]
    try:
        text = data.decode("utf-8")
    except UnicodeDecodeError as e:
        return [(path.name, None, None, f"not UTF-8: {e}")]
    if path.suffix == ".toml":
        try:
            return [(path.name, "policy", toml_value(tomllib.loads(text)), None)]
        except (ValueError, RecursionError, tomllib.TOMLDecodeError) as e:
            return [(path.name, None, None, str(e))]
    if path.suffix == ".json":
        lines = [(path.name, text)]
    else:
        raw = text.split("\n")
        if raw and raw[-1] == "":
            raw.pop()
        lines = [(f"{path.name}:{n}", line) for n, line in enumerate(raw, 1)]
    out = []
    for label, line in lines:
        if not line.strip():
            out.append((label, None, None, "an empty line"))
            continue
        try:
            doc = parse_json(line)
        except (ValueError, RecursionError) as e:  # RecursionError: very deep nesting
            out.append((label, None, None, str(e)))
            continue
        out.append((label, select(base, doc), doc, None))
    return out


def newer(text, minor):
    """True if the minor version `text` (digits, no leading zero) is higher
    than `minor`. No int() of the text: a very long text has no limit."""
    k = str(minor)
    return (len(text), text) > (len(k), k)


def verdict(validators, minor, name, doc):
    """None if the document passes, else the reason. With minor None, only
    the schemas decide (the self-test checks the schemas)."""
    if name is None:
        return "no form of the five forms"
    if name not in validators:
        return "no schema"
    if isinstance(doc, dict) and isinstance(doc.get("model"), str):
        m = MODEL.match(doc["model"])
        if m and minor is not None and newer(m.group(1), minor):
            return "newer than the kit"
    errors = sorted(validators[name].iter_errors(doc), key=lambda e: list(e.absolute_path))
    if errors:
        e = errors[0]
        where = "/" + "/".join(str(p) for p in e.absolute_path)
        return f"{e.message} at {where}"
    return None


def check_file(validators, minor, path):
    """Each failure of one file, as text, and the count of documents."""
    fails = []
    docs = documents(path)
    for label, name, doc, error in docs:
        reason = error or verdict(validators, minor, name, doc)
        if reason:
            fails.append(f"{label}: {reason}")
    return fails, len(docs)


def files_of(folder):
    return sorted(p for p in folder.iterdir() if p.is_file() and p.suffix in SUFFIXES)


def self_test(validators, minor):
    bad = 0
    for sub, expect in (("examples", True), ("examples/invalid", False)):
        for path in files_of(SCHEMAS / sub):
            fails, _ = check_file(validators, None, path)
            if (not fails) != expect:
                bad += 1
                want = "pass" if expect else "fail"
                print(f"{sub}/{path.name}: must {want}" + (f" ({fails[0]})" if fails else ""))
    return 1 if bad else 0


def check(validators, minor, folder):
    if not folder.is_dir():
        raise KitError(f"{folder} is not a folder")
    count = 0
    failed = False
    for path in files_of(folder):
        fails, n = check_file(validators, minor, path)
        count += n
        for line in fails:
            failed = True
            print(f"fail {line}")
    if count == 0 and not failed:
        print(f"fail {folder}: no documents")
        return 1
    return 1 if failed else 0


def main(argv):
    try:
        if argv == ["--version"]:
            print(kit_version()[0])
            return 0
        _, _, minor = kit_version()
        if argv == ["self-test"]:
            return self_test(load_validators(), minor)
        if len(argv) == 2 and argv[0] == "check":
            return check(load_validators(), minor, Path(argv[1]))
        print("usage: run.py self-test | run.py check <folder> | run.py --version", file=sys.stderr)
        return 2
    except Exception as e:
        # Each document error is a verdict inside check_file, so an
        # exception here is the kit or the file system (a missing VERSION,
        # a schema that is not JSON, a read error): exit 2.
        print(f"error: {type(e).__name__}: {e}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
