#!/usr/bin/env python3
"""A sample producer of the Iris open model, with no Iris code.

It reads each .eml file in mail/ (beside this file) and writes one document
for each message that a fixed rule matches:

- a message with a List-Id header: a feed item (feed-<n>.json);
- a message with a line that has the word "code" and then six digits:
  a signal of the kind "code" (signal-<n>.json).

It uses only the Python standard library. The rules are small on purpose:
the producer proves that a program outside Iris can write documents that
pass the kit (docs/spec/04-open-model-versions.md, section 5.3).

usage: produce.py <out-folder>
It refuses an <out-folder> or <out-folder>.partial that exists, writes into
<out-folder>.partial, and renames it to <out-folder> after the last file.
"""

import datetime
import email
import email.policy
import email.utils
import json
import os
import re
import sys
from pathlib import Path

MAIL = Path(__file__).resolve().parent / "mail"
MODEL = "iris/1.0"
CODE = re.compile(r"\bcode\b[^0-9\n]*\b([0-9]{6})\b", re.IGNORECASE)


def utc(dt):
    return dt.astimezone(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")


def documents():
    """Each (file name, document), in the order of the mail file names."""
    senders = {}
    counts = {"feed": 0, "signal": 0}
    out = []
    for n, path in enumerate(sorted(MAIL.glob("*.eml")), 1):
        msg = email.message_from_bytes(path.read_bytes(), policy=email.policy.default)
        name, address = email.utils.parseaddr(str(msg["From"]))
        address = address.lower()
        sender = senders.setdefault(address, f"p-{len(senders) + 1}")
        party = {"kind": "sender", "sender": sender, "address": address}
        if name:
            party["name"] = name
        date = email.utils.parsedate_to_datetime(str(msg["Date"]))
        subject = str(msg["Subject"] or "").strip()
        body = msg.get_body(preferencelist=("plain",))
        text = body.get_content() if body else ""
        base = {"model": MODEL, "created": utc(date), "updated": utc(date)}
        source = {"message": f"m-{n}", "thread": f"t-{n}", "role": "origin"}
        match = CODE.search(text)
        if msg["List-Id"] is not None and subject:
            counts["feed"] += 1
            k = counts["feed"]
            doc = {**base, "id": f"f-{k}", "form": "feed", "status": "new",
                   "title": subject[:200], "publisher": party,
                   "list": str(msg["List-Id"])[:500], "published": utc(date),
                   "sources": [{**source, "quote": subject[:500]}],
                   "derived": {"by": "rule", "rule": "list-id"}}
            out.append((f"feed-{k}.json", doc))
        elif match:
            counts["signal"] += 1
            k = counts["signal"]
            line = next(l for l in text.splitlines() if match.group(1) in l).strip()
            doc = {**base, "id": f"s-{k}", "form": "signal", "status": "active",
                   "title": (subject or "A code")[:200], "kind": "code", "issuer": party,
                   "code": match.group(1),
                   "expires": utc(date + datetime.timedelta(minutes=15)),
                   "sources": [{**source, "quote": line[:500]}],
                   "derived": {"by": "rule", "rule": "code"}}
            out.append((f"signal-{k}.json", doc))
    return out


def main(argv):
    if len(argv) != 1:
        print("usage: produce.py <out-folder>", file=sys.stderr)
        return 2
    out = Path(argv[0])
    partial = Path(f"{out}.partial")
    for path in (out, partial):
        if os.path.lexists(path):
            print(f"error: {path} exists; remove it or name a new folder", file=sys.stderr)
            return 1
    try:
        partial.mkdir()
        for name, doc in documents():
            # "x": never replace a file.
            with open(partial / name, "x", encoding="utf-8") as f:
                f.write(json.dumps(doc, indent=2, ensure_ascii=False) + "\n")
        partial.rename(out)
    except OSError as e:
        print(f"error: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
