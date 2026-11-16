// The jobs of the agents (R4-09): the route, the words of each step, and server renders with the fixtures.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, render } from "./render.mjs";

const fx = await load("/dev/fixtures.ts");
const j = await load("/src/jobs.ts");
const { parse, href, NAV } = await load("/src/route.ts");
const text = (html) => html.replace(/<!--[^>]*-->/g, "").replace(/<[^>]+>/g, " ").replace(/\s+/g, " ");

test("the routes of the jobs", () => {
  assert.deepEqual(parse("#/jobs"), { view: "jobs" });
  assert.deepEqual(parse("#/jobs/j-12"), { view: "job", id: "j-12" });
  assert.equal(href.job("j-12"), "#/jobs/j-12");
  assert.ok(NAV.some(([link]) => link === "#/jobs"));
});

test("each step of a job comes in time order, the events and the changesets together", () => {
  const steps = j.steps(fx.jobDetails["j-12"]);
  const times = steps.map((s) => Date.parse(s.at));
  assert.deepEqual(times, [...times].sort((a, b) => a - b));
  assert.equal(steps.length, 8);
  assert.deepEqual(
    steps.map((s) => s.key),
    ["e0", "cs-31", "e1", "e2", "e3", "e4", "cs-34", "e5"],
  );
  assert.equal(steps[0].text, "You gave the goal.");
  assert.equal(steps[1].text, "The agent asked for 3 changes: 3 done.");
  assert.equal(steps[1].link, undefined, "a closed changeset has no review link");
  assert.equal(steps[3].text, "A reply came. The agent works again.");
  assert.equal(steps[4].quote, "Dana sent a version with a new clause 7 (payment in 45 days, not 30). Accept it?");
  assert.equal(steps[5].text, "You answered:");
  assert.deepEqual(steps[6].link, { href: "#/review", text: "Review" });
});

test("generated timelines: each event and each changeset shows once, in time order", () => {
  let seed = 7;
  const rand = (n) => ((seed = (seed * 1103515245 + 12345) % 2147483648), seed % n);
  for (let round = 0; round < 200; round++) {
    const events = Array.from({ length: 1 + rand(6) }, (_, i) => ({ at: new Date(1e12 + i * 1000).toISOString(), state: "working", by: "person" }));
    const changesets = Array.from({ length: rand(5) }, (_, i) => ({ id: `cs-${i}`, at: new Date(1e12 + rand(8000)).toISOString(), state: "open", why: null, actions: { applied: 1 } }));
    changesets.sort((a, b) => Date.parse(a.at) - Date.parse(b.at));
    const steps = j.steps({ job: { ...fx.jobs["j-15"], events }, changesets, threads: {} });
    assert.equal(steps.length, events.length + changesets.length);
    const times = steps.map((s) => Date.parse(s.at));
    assert.deepEqual(times, [...times].sort((a, b) => a - b));
  }
});

test("the state now in words", () => {
  const { jobs, jobDetails } = fx;
  assert.equal(j.now(jobs["j-15"]), "The agent works on it.");
  assert.equal(j.now(jobs["j-14"]), "The agent asks you a question.");
  assert.equal(j.now(jobs["j-12"], jobDetails["j-12"].threads), "Waits for a reply from Dana Whitfield, until Wed 18 Nov.");
  assert.equal(j.now(jobs["j-12"]), "Waits for a reply, until Wed 18 Nov.");
  assert.equal(j.now({ ...jobs["j-12"], wait: { until: jobs["j-12"].wait.until } }), "Waits, until Wed 18 Nov.");
  assert.equal(j.now(jobs["j-9"]), "Done Fri 13 Nov.");
  assert.equal(j.now({ ...jobs["j-9"], end: "cancel" }), "Cancelled Fri 13 Nov.");
});

test("a due date has a tone near its day and after it, and an ended job has none", () => {
  const job = { ...fx.jobs["j-12"], due: "2026-11-20T00:00:00.000Z" };
  assert.equal(j.due(job, new Date("2026-11-10T12:00:00Z")).tone, "");
  assert.equal(j.due(job, new Date("2026-11-18T12:00:00Z")).tone, "soon");
  assert.equal(j.due(job, new Date("2026-11-21T12:00:00Z")).tone, "late");
  assert.equal(j.due({ ...job, state: "done" }), null);
  assert.equal(j.due(fx.jobs["j-15"]), null);
});

test("a wait on a thread that is gone says so, with no link", () => {
  const d = structuredClone(fx.jobDetails["j-12"]);
  d.threads["t-8841"] = { id: "t-8841", gone: true };
  const wait = j.steps(d).find((s) => s.key === "e1");
  assert.equal(wait.mail, undefined);
  assert.match(wait.text, /The mail is gone\./);
});

test("the job page shows the goal, the state and each step; agent text stays text", async () => {
  const d = structuredClone(fx.jobDetails["j-14"]);
  d.job.question = "<img src=x onerror=alert(1)> Pay it?";
  const html = await render("/src/views/Job.svelte", { detail: d });
  assert.ok(!html.includes("<img"), "the question is escaped");
  assert.ok(text(html).includes("Pay the Q4 invoice from Brightline."));
  assert.ok(text(html).includes("The agent asks you"));
  const page = text(await render("/src/views/Job.svelte", { detail: fx.jobDetails["j-12"] }));
  for (const words of ["Each step", "You gave the goal.", "Waits for a reply from Dana Whitfield", "Re: Northwind contract, final version", "Accept 45 days."]) {
    assert.ok(page.includes(words), words);
  }
});

test("the list puts the jobs that ask you first, and the ended jobs last", async () => {
  const page = (state) => fx.answer(`/jobs?state=${state}`);
  const html = text(await render("/src/views/Jobs.svelte", { open: page("open"), done: page("done") }));
  const order = ["Asks you", "Pay the Q4 invoice", "Working", "Book a dentist", "Waiting", "Get the signed contract", "Ended", "Cancel the gym"].map((w) => html.indexOf(w));
  assert.ok(order.every((n) => n >= 0), String(order));
  assert.deepEqual(order, [...order].sort((a, b) => a - b));
  const none = text(await render("/src/views/Jobs.svelte", { open: { jobs: [], next: null }, done: { jobs: [], next: null } }));
  assert.ok(none.includes("No job yet."));
});
