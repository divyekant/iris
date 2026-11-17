"""Tests of the project site in docs/ (GitHub Pages from develop /docs).

Run: python3 -m unittest discover -s scripts/site -v

The site is static HTML. These tests make sure that:
- each local link and each fragment goes to a file and an id that exist;
- each GitHub link into this repository names a path that exists;
- each sample on a page is the same text as its file in the kit;
- the documents page lists each schema, with its required fields;
- the code sample of the reader rules gives the results of specification 4;
- the home page and the privacy page that the Google check uses stay.
"""

import html
import json
import random
import re
import sys
import unittest
from html.parser import HTMLParser
from pathlib import Path
from urllib.parse import urldefrag

ROOT = Path(__file__).resolve().parents[2]
DOCS = ROOT / "docs"
SCHEMAS = DOCS / "spec" / "schema"
EXAMPLES = SCHEMAS / "examples"
REPO = "https://github.com/divyekant/iris/"
FORMS = ("commitment", "conversation", "record", "feed", "signal")


class Page(HTMLParser):
    """The links, the ids and the marked blocks of one HTML page."""

    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.links, self.ids = [], set()
        self.blocks = []  # (attrs, text) of each <pre> or <ul> with a data- mark
        self._open = None
        self._text = []

    def handle_starttag(self, tag, attrs):
        a = dict(attrs)
        if "id" in a:
            self.ids.add(a["id"])
        for key in ("href", "src"):
            if a.get(key):
                self.links.append(a[key])
        if tag in ("pre", "ul") and any(k.startswith("data-") for k in a):
            self._open, self._text = (tag, a), []

    def handle_endtag(self, tag):
        if self._open and tag == self._open[0]:
            self.blocks.append((self._open[1], "".join(self._text)))
            self._open = None

    def handle_data(self, data):
        if self._open:
            self._text.append(data)


def pages():
    out = {}
    for path in sorted([*DOCS.glob("*.html"), *DOCS.glob("guide/*.html")]):
        p = Page()
        p.feed(path.read_text(encoding="utf-8"))
        out[path] = p
    return out


def schema_names():
    return sorted(p.name.split(".")[0] for p in SCHEMAS.glob("*.schema.json"))


def required_fields(name):
    schema = json.loads((SCHEMAS / f"{name}.schema.json").read_text(encoding="utf-8"))
    fields = set(schema.get("required", []))
    if name in FORMS:
        common = json.loads((SCHEMAS / "common.schema.json").read_text(encoding="utf-8"))
        fields |= set(common["$defs"]["item"]["required"])
    return fields


class SiteTest(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.pages = pages()

    def test_the_pages_of_the_google_check_stay(self):
        home, privacy = DOCS / "index.html", DOCS / "privacy.html"
        self.assertIn(home, self.pages)
        self.assertIn(privacy, self.pages)
        self.assertIn("privacy.html", self.pages[home].links)
        text = privacy.read_text(encoding="utf-8")
        self.assertIn("gmail.readonly", text)
        self.assertIn("Google API Services User Data Policy", text)

    def test_the_home_page_links_the_guide(self):
        self.assertIn("guide/index.html", self.pages[DOCS / "index.html"].links)

    def test_each_local_link_and_fragment_exists(self):
        for path, page in self.pages.items():
            for link in page.links:
                if re.match(r"^[a-z]+:", link):
                    continue
                target, frag = urldefrag(link)
                dest = (path.parent / target).resolve() if target else path
                with self.subTest(page=path.name, link=link):
                    self.assertTrue(dest.is_file(), f"{dest} is not a file")
                    self.assertTrue(dest.is_relative_to(DOCS), "the link leaves docs/")
                    if frag:
                        self.assertIn(frag, self.pages[dest].ids)

    def test_each_link_into_the_repository_names_a_path_that_exists(self):
        seen = 0
        for path, page in self.pages.items():
            for link in page.links:
                m = re.match(re.escape(REPO) + r"(blob|tree)/develop/([^#?]+)", link)
                if not m:
                    continue
                seen += 1
                with self.subTest(page=path.name, link=link):
                    self.assertTrue((ROOT / m.group(2)).exists(), m.group(2))
        self.assertGreater(seen, 0)

    def test_each_sample_is_the_text_of_its_file(self):
        seen = set()
        for path, page in self.pages.items():
            for attrs, text in page.blocks:
                name = attrs.get("data-sample")
                if not name:
                    continue
                seen.add(name)
                with self.subTest(page=path.name, sample=name):
                    want = (EXAMPLES / name).read_text(encoding="utf-8")
                    self.assertEqual(text.strip("\n"), want.strip("\n"))
        self.assertGreater(len(seen), 0)

    def test_the_documents_page_lists_each_schema_with_its_required_fields(self):
        page = self.pages[DOCS / "guide" / "documents.html"]
        lists = {a["data-required"]: t for a, t in page.blocks if "data-required" in a}
        samples = {a["data-sample"] for a, _ in page.blocks if "data-sample" in a}
        for name in schema_names():
            if name == "common":
                continue
            with self.subTest(schema=name):
                self.assertIn(name, page.ids)
                self.assertIn(name, lists)
                listed = set(re.findall(r"\S+", lists[name]))
                self.assertEqual(listed, required_fields(name))
                self.assertTrue(any(s.split("-")[0].split(".")[0] == name for s in samples))

    def test_the_model_value_of_the_guide_is_the_kit_version(self):
        version = (DOCS / "spec" / "VERSION").read_text(encoding="utf-8").strip()
        text = (DOCS / "guide" / "index.html").read_text(encoding="utf-8")
        self.assertIn(f"iris/{version}", text)

    def test_the_reader_sample_code_follows_specification_4(self):
        page = self.pages[DOCS / "guide" / "read.html"]
        code = [t for a, t in page.blocks if a.get("data-code") == "minor"]
        self.assertEqual(len(code), 1)
        scope = {}
        exec(html.unescape(code[0]), scope)
        check = scope["check_model"]
        cases = {
            "iris/1.0": "ok",
            "iris/1.3": "ok",
            "iris/1.4": "show only",
            "iris/1.10": "show only",
            "iris/1.01": "refuse",
            "iris/2.0": "refuse",
            "iris/1": "refuse",
            "iris/1.0\n": "refuse",
            "IRIS/1.0": "refuse",
            None: "refuse",
            10: "refuse",
        }
        for model, want in cases.items():
            with self.subTest(model=model):
                self.assertEqual(check({"model": model}, 3), want)
        self.assertEqual(check({}, 3), "refuse")
        for value in ([], "iris/1.0", 1, None, True, [{"model": "iris/1.0"}]):
            with self.subTest(value=value):
                self.assertEqual(check(value, 3), "refuse")

    def test_the_reader_sample_code_agrees_with_the_rule_on_random_input(self):
        page = self.pages[DOCS / "guide" / "read.html"]
        code = [t for a, t in page.blocks if a.get("data-code") == "minor"][0]
        scope = {}
        exec(html.unescape(code), scope)
        check = scope["check_model"]
        rng = random.Random(8)
        parts = ["iris", "IRIS", "/", "1", "2", ".", "0", "00", "7", "\n", " ", "x", "-1"]

        def model():
            kind = rng.randrange(3)
            if kind == 0:  # A valid model, with a minor of up to 6000 digits.
                n = rng.choice([1, 2, 3, 40, 6000])
                digits = str(rng.randrange(1, 10)) + "".join(rng.choice("0123456789") for _ in range(n - 1))
                return "iris/1." + rng.choice([digits, "0", str(rng.randrange(10))])
            if kind == 1:
                return "".join(rng.choice(parts) for _ in range(rng.randrange(8)))
            return rng.choice([None, 3, 1.0, [], {}, True])

        def oracle(doc, k):
            m = doc.get("model") if isinstance(doc, dict) else None
            if not isinstance(m, str) or not re.fullmatch(r"iris/1\.(0|[1-9][0-9]*)", m):
                return "refuse"
            # The oracle uses int() with no digit limit. The code under test
            # runs with the default limit of Python (4300 digits from 3.11).
            limit = getattr(sys, "get_int_max_str_digits", lambda: 0)()
            setter = getattr(sys, "set_int_max_str_digits", lambda n: None)
            setter(0)
            try:
                minor = int(m[len("iris/1."):])
            finally:
                setter(limit)
            return "show only" if minor > k else "ok"

        for _ in range(3000):
            k = rng.choice([0, 3, 9, 10, 123])
            doc = rng.choice([{"model": model()}, model(), {}, [model()]])
            with self.subTest(doc=repr(doc)[:80], k=k):
                self.assertEqual(check(doc, k), oracle(doc, k))


if __name__ == "__main__":
    unittest.main()
