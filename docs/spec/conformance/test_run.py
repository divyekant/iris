"""Tests of run.py and producer/produce.py: the failure list of
docs/specs/2026-11-17-open-model-v1.md. Run: python3 -m unittest test_run
(from this folder, with the libraries of requirements.txt)."""

import contextlib
import io
import json
import os
import random
import re
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import run  # noqa: E402

GOOD = json.loads((HERE.parent / "schema/examples/signal-code.json").read_text())


def check(files):
    """run.py check on a new folder with these files: (exit code, output)."""
    with tempfile.TemporaryDirectory() as d:
        for name, data in files.items():
            p = Path(d) / name
            if data is None:
                p.mkdir()
            else:
                p.write_bytes(data if isinstance(data, bytes) else data.encode())
        out = io.StringIO()
        with contextlib.redirect_stdout(out):
            code = run.main(["check", d])
        return code, out.getvalue()


class Runner(unittest.TestCase):
    def test_a_good_document_passes(self):
        self.assertEqual(check({"signal-a.json": json.dumps(GOOD)})[0], 0)

    def test_each_bad_input_fails_and_names_the_file(self):
        bad = {
            "signal-text.json": "not json",
            "signal-bytes.json": b"\xff\xfe",
            "signal-dup.json": '{"model": "iris/1.0", "model": "iris/1.0"}',
            "signal-nan.json": '{"x": NaN}',
            "letter-1.json": json.dumps(GOOD),
            "items.jsonl": json.dumps({"form": "memo"}) + "\n",
            "signal-lines.jsonl": json.dumps(GOOD) + "\n\n" + json.dumps(GOOD) + "\n",
            "policy.toml": "version = 2\nmode = \"apply\"\n",
            "signal-deep.json": "[" * 100000 + "]" * 100000,
        }
        for name, data in bad.items():
            code, out = check({name: data})
            self.assertEqual(code, 1, name)
            self.assertIn(name, out, name)

    def test_items_select_the_schema_by_form(self):
        lines = (HERE.parent / "schema/examples/items.jsonl").read_text()
        self.assertEqual(check({"items.jsonl": lines})[0], 0)

    def test_an_empty_folder_and_a_subfolder_have_no_documents(self):
        self.assertEqual(check({})[0], 1)
        self.assertEqual(check({"signal-sub.json": None})[0], 1)

    def test_a_newer_minor_version_fails_check(self):
        doc = dict(GOOD, model="iris/1.7")
        code, out = check({"signal-new.json": json.dumps(doc)})
        self.assertEqual(code, 1)
        self.assertIn("newer than the kit", out)

    def test_a_very_long_minor_version_is_newer_not_a_traceback(self):
        doc = dict(GOOD, model="iris/1." + "9" * 5000)
        code, out = check({"signal-long.json": json.dumps(doc)})
        self.assertEqual(code, 1)
        self.assertIn("newer than the kit", out)
        self.assertTrue(run.newer("10", 9) and not run.newer("0", 0))

    def test_an_impossible_date_fails(self):
        doc = dict(GOOD, expires="2026-02-30T09:15:00Z")
        self.assertEqual(check({"signal-date.json": json.dumps(doc)})[0], 1)

    def test_self_test_passes(self):
        with contextlib.redirect_stdout(io.StringIO()):
            self.assertEqual(run.main(["self-test"]), 0)

    def test_a_ref_outside_the_kit_is_exit_2(self):
        with tempfile.TemporaryDirectory() as d:
            for p in (HERE.parent / "schema").glob("*.schema.json"):
                (Path(d) / p.name).write_text(p.read_text())
            s = Path(d) / "feed.schema.json"
            s.write_text(s.read_text().replace("urn:iris:model:1:common#", "urn:iris:model:1:gone#", 1))
            old = run.SCHEMAS
            run.SCHEMAS = Path(d)
            try:
                with self.assertRaises(run.KitError):
                    run.load_validators()
            finally:
                run.SCHEMAS = old

    def test_each_jsonl_line_gets_its_own_verdict(self):
        # Property test: a random mix of good and bad lines; check names
        # each bad line and no good line.
        good = json.dumps(GOOD)
        bad = ["not json", "{\"form\": \"memo\"}", "", "{\"a\": 1, \"a\": 2}",
               json.dumps(dict(GOOD, status="lost"))]
        rng = random.Random(7)
        for _ in range(60):
            lines, want = [], set()
            for n in range(1, rng.randint(1, 8) + 1):
                if rng.random() < 0.5:
                    lines.append(good)
                else:
                    lines.append(rng.choice(bad))
                    want.add(n)
            code, out = check({"items.jsonl": "\n".join(lines) + "\n"})
            got = {int(m) for m in re.findall(r"items\.jsonl:(\d+):", out)}
            self.assertEqual(got, want, lines)
            self.assertEqual(code, 1 if want else 0)

    def test_a_file_that_cannot_be_read_is_exit_2(self):
        def refuse(self):
            raise PermissionError("no access")
        old = Path.read_bytes
        Path.read_bytes = refuse
        try:
            with contextlib.redirect_stderr(io.StringIO()):
                self.assertEqual(check({"signal-a.json": "{}"})[0], 2)
        finally:
            Path.read_bytes = old

    def test_an_incomplete_kit_is_exit_2(self):
        with tempfile.TemporaryDirectory() as d:
            spec, schemas = run.SPEC, run.SCHEMAS
            try:
                # No VERSION file.
                run.SPEC = Path(d)
                with contextlib.redirect_stderr(io.StringIO()):
                    self.assertEqual(run.main(["--version"]), 2)
                    self.assertEqual(run.main(["self-test"]), 2)
                # A schema that is not JSON.
                run.SPEC = spec
                run.SCHEMAS = Path(d)
                (Path(d) / "feed.schema.json").write_text("{")
                with contextlib.redirect_stderr(io.StringIO()):
                    self.assertEqual(run.main(["self-test"]), 2)
                # A schema that is not there.
                for p in (HERE.parent / "schema").glob("*.schema.json"):
                    if p.name != "feed.schema.json":
                        (Path(d) / p.name).write_text(p.read_text())
                (Path(d) / "feed.schema.json").unlink()
                with contextlib.redirect_stderr(io.StringIO()):
                    self.assertEqual(run.main(["check", str(HERE)]), 2)
            finally:
                run.SPEC, run.SCHEMAS = spec, schemas

    def test_depth_matches_the_parsed_document(self):
        """For random JSON with brackets and quotes in strings, the scan
        before the parse gives the depth of the parsed document."""

        def real(v):
            if isinstance(v, dict):
                return 1 + max(map(real, v.values()), default=0)
            if isinstance(v, list):
                return 1 + max(map(real, v), default=0)
            return 0

        def value(rng, level):
            k = rng.randrange(5 if level < 12 else 3)
            if k == 0:
                return "".join(rng.choice('ab[]{}"\\') for _ in range(rng.randrange(6)))
            if k == 1:
                return rng.randrange(100)
            if k == 2:
                return None
            if k == 3:
                return [value(rng, level + 1) for _ in range(rng.randrange(4))]
            return {str(i) + "]": value(rng, level + 1) for i in range(rng.randrange(4))}

        rng = random.Random(7)
        for _ in range(2000):
            doc = value(rng, 0)
            self.assertEqual(run.depth(json.dumps(doc)), real(doc), doc)

    def test_nesting_past_the_limit_fails_with_a_reason(self):
        ok = "[" * run.MAX_DEPTH + "]" * run.MAX_DEPTH
        self.assertEqual(run.depth(ok), run.MAX_DEPTH)
        code, out = check({"signal-deep.json": "[" + ok + "]"})
        self.assertEqual(code, 1, out)
        self.assertIn("nesting deeper than", out)

    def test_usage_is_exit_2(self):
        with contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(run.main(["nothing"]), 2)


class Producer(unittest.TestCase):
    def produce(self, out):
        return subprocess.run(
            [sys.executable, "-I", str(HERE / "producer/produce.py"), str(out)],
            capture_output=True,
        ).returncode

    def test_the_producer_passes_the_kit(self):
        with tempfile.TemporaryDirectory() as d:
            out = Path(d) / "out"
            self.assertEqual(self.produce(out), 0)
            names = sorted(p.name for p in out.iterdir())
            self.assertEqual(names, ["feed-1.json", "signal-1.json"])
            self.assertFalse(Path(f"{out}.partial").exists())
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(run.main(["check", str(out)]), 0)

    def test_the_producer_never_writes_over_a_path(self):
        with tempfile.TemporaryDirectory() as d:
            for name in ("out", "out.partial"):
                p = Path(d) / name
                p.mkdir()
                (p / "keep").write_text("x")
                self.assertEqual(self.produce(Path(d) / "out"), 1)
                self.assertEqual(os.listdir(p), ["keep"])
                (p / "keep").unlink()
                p.rmdir()


if __name__ == "__main__":
    unittest.main()
