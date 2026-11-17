"""Tests of run.py and producer/produce.py: the failure list of
docs/specs/2026-11-17-open-model-v1.md. Run: python3 -m unittest test_run
(from this folder, with the libraries of requirements.txt)."""

import contextlib
import io
import json
import os
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
