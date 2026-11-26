// "Handle this" (R4-10): the due date of a start, and the grouping of the mail.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load } from "./render.mjs";

const s = await load("/src/start.ts");
const act = await load("/src/act.ts");

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

test("the due date of a job is a day of the owner zone, not of the browser (issue 892)", () => {
  const { today } = act;
  const zone = process.env.TZ;
  try {
    // The browser is in Los Angeles, the owner in Tokyo.
    process.env.TZ = "America/Los_Angeles";
    const timed = { due: { at: "2026-11-20T17:00:00Z" } }; // 09:00 on 20 Nov in Los Angeles, 02:00 on 21 Nov in Tokyo.
    assert.equal(s.dueDate(timed, "Asia/Tokyo"), "2026-11-21");
    assert.equal(s.dueValue(timed, "2026-11-21", "Asia/Tokyo"), "2026-11-20T17:00:00Z");
    assert.equal(s.endOfDay("2026-11-21", "Asia/Tokyo"), "2026-11-21T23:59:59.999+09:00");
    assert.equal(s.keepDue("2026-11-20T17:00:00Z", "2026-11-21", "Asia/Tokyo"), "2026-11-20T17:00:00Z");
    assert.equal(s.keepDue("2026-11-20T17:00:00Z", "2026-11-20", "Asia/Tokyo"), "2026-11-20T23:59:59.999+09:00");
    // No zone, or a zone that the browser does not know: the browser day, as before.
    assert.equal(s.dueDate(timed, null), "2026-11-20");
    assert.equal(s.endOfDay("2026-11-21", "Not/AZone"), s.endOfDay("2026-11-21", null));
    // Property: in each zone and on each day of a year, the end is the last millisecond of the owner day.
    for (const tz of [...ZONES, "Europe/Berlin", "America/Havana", "America/Santiago"]) {
      for (let day = 0; day < 366; day++) {
        const field = new Date(Date.UTC(2026, 0, 1 + day)).toISOString().slice(0, 10);
        const out = s.endOfDay(field, tz);
        assert.match(out, /^\d{4}-\d{2}-\d{2}T23:59:59\.999[+-]\d{2}:\d{2}$/, `${tz} ${out}`);
        const t = Date.parse(out);
        assert.equal(today(new Date(t), tz), field, `${tz} ${out}`);
        assert.notEqual(today(new Date(t + 1), tz), field, `${tz} ${out}`);
      }
    }
  } finally {
    process.env.TZ = zone;
  }
});

test("endOfDay refuses a day that the owner zone skipped, and keeps the days around it (issue 987)", () => {
  const { today } = act;
  // Pacific/Apia skipped 2011-12-30, Pacific/Kwajalein skipped 1993-08-21.
  for (const [tz, gone] of [["Pacific/Apia", "2011-12-30"], ["Pacific/Kwajalein", "1993-08-21"]]) {
    assert.throws(() => s.endOfDay(gone, tz), /does not exist in the time zone/, `${tz} ${gone}`);
    assert.throws(() => s.keepDue(undefined, gone, tz), /does not exist/);
    assert.throws(() => s.dueValue({}, gone, tz), /does not exist/);
    for (const near of [-2, -1, 1, 2]) {
      const t = new Date(Date.parse(`${gone}T12:00:00Z`) + near * 86_400_000).toISOString().slice(0, 10);
      assert.equal(today(new Date(Date.parse(s.endOfDay(t, tz))), tz), t, `${tz} ${t}`);
    }
  }
});

test("with no owner zone, endOfDay refuses a day that the browser zone skipped (issue 987)", () => {
  const zone = process.env.TZ;
  try {
    process.env.TZ = "Pacific/Apia";
    assert.throws(() => s.endOfDay("2011-12-30", null), /2011-12-30 does not exist in the time zone of this browser/);
    assert.match(s.endOfDay("2011-12-29", null), /^2011-12-29T23:59:59\.999-10:00$/);
    assert.match(s.endOfDay("2011-12-31", null), /^2011-12-31T23:59:59\.999\+14:00$/);
  } finally {
    process.env.TZ = zone;
  }
});
