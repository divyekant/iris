// "Handle this" (R4-10): the due date of a start, and the grouping of the mail.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load } from "./render.mjs";

const s = await load("/src/start.ts");

const ZONES = ["UTC", "America/Los_Angeles", "Asia/Kolkata", "Australia/Lord_Howe", "Pacific/Kiritimati", "America/St_Johns"];

test("endOfDay gives the last millisecond of the local day, in each zone and on each day of a year", () => {
  const zone = process.env.TZ;
  try {
    for (const tz of ZONES) {
      process.env.TZ = tz;
      for (let day = 0; day < 366; day++) {
        const d = new Date(Date.UTC(2026, 0, 1 + day));
        const field = d.toISOString().slice(0, 10);
        const out = s.endOfDay(field);
        assert.match(out, /^\d{4}-\d{2}-\d{2}T23:59:59\.999[+-]\d{2}:\d{2}$/, `${tz} ${out}`);
        const t = new Date(out);
        // The instant is in the picked local day, and one millisecond later is the next day.
        const [y, m, dd] = field.split("-").map(Number);
        assert.deepEqual([t.getFullYear(), t.getMonth() + 1, t.getDate(), t.getHours(), t.getMinutes()], [y, m, dd, 23, 59], `${tz} ${field} ${out}`);
        assert.equal(new Date(t.getTime() + 1).getHours(), 0, `${tz} ${field}`);
      }
    }
  } finally {
    process.env.TZ = zone;
  }
});

test("dueValue keeps the instant of a commitment while its day stays, else the end of the picked day", () => {
  const timed = { due: { at: "2026-11-20T17:00:00Z" } };
  const day = s.dueDate(timed);
  assert.equal(s.dueValue(timed, day), "2026-11-20T17:00:00Z");
  assert.equal(s.dueValue(timed, ""), null);
  assert.equal(s.dueValue(timed, "2026-11-21"), s.endOfDay("2026-11-21"));
  const dated = { due: { date: "2026-11-17", strict: false } };
  assert.equal(s.dueDate(dated), "2026-11-17");
  assert.equal(s.dueValue(dated, "2026-11-17"), s.endOfDay("2026-11-17"));
  assert.equal(s.dueDate({}), "");
  assert.equal(s.dueValue({}, ""), null);
});

test("canStart: an open commitment, or an active or quiet conversation", () => {
  const yes = [["commitment", "open"], ["conversation", "active"], ["conversation", "quiet"]];
  for (const form of ["commitment", "conversation", "record", "feed", "signal"])
    for (const status of ["open", "done", "declined", "gone", "active", "quiet", "muted", "ended"])
      assert.equal(s.canStart({ form, status }), yes.some(([f, st]) => f === form && st === status), `${form} ${status}`);
});

test("byThread keeps the order of the list and groups each run of one thread", () => {
  const m = (id, thread, subject) => ({ id, thread, subject, readable: true });
  const list = [m("m-1", "t-1", "A"), m("m-2", "t-1", "Re: A"), m("m-3", "t-2", null)];
  const groups = s.byThread(list);
  assert.deepEqual(groups.map((g) => [g.thread, g.subject, g.messages.map((x) => x.id)]), [["t-1", "A", ["m-1", "m-2"]], ["t-2", "(no subject)", ["m-3"]]]);
  assert.equal(s.reads(list), "3 messages in 2 threads");
  assert.equal(s.reads([list[0]]), "1 message in 1 thread");
});

test("subjectOf names an empty or blank subject (no subject), as the workspace does (#413)", () => {
  for (const blank of [null, undefined, "", "   ", "\t\n"]) assert.equal(s.subjectOf({ subject: blank }), "(no subject)", JSON.stringify(blank));
  assert.equal(s.subjectOf({ subject: "  Q4 numbers " }), "Q4 numbers");
  const groups = s.byThread([{ id: "m-1", thread: "t-1", subject: "  ", readable: true }]);
  assert.equal(groups[0].subject, "(no subject)");
});
