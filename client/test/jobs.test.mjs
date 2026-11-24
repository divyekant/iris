// The jobs of the agents (R4-09): the route, the words of each step, and server renders with the fixtures.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, render } from "./render.mjs";

const fx = await load("/dev/fixtures.ts");
const j = await load("/src/jobs.ts");
const { access } = await load("/src/access.svelte.ts");
const { parse, href, NAV, JOB_ID } = await load("/src/route.ts");
const text = (html) => html.replace(/<!--[^>]*-->/g, "").replace(/<[^>]+>/g, " ").replace(/\s+/g, " ");

test("the routes of the jobs", () => {
  assert.deepEqual(parse("#/jobs"), { view: "jobs" });
  assert.deepEqual(parse("#/jobs/j-12"), { view: "job", id: "j-12" });
  assert.equal(href.job("j-12"), "#/jobs/j-12");
  assert.ok(NAV.some(([link]) => link === "#/jobs"));
  assert.deepEqual(parse("#/jobs/"), { view: "jobs" });
  for (const bad of ["#/jobs/j-12/extra", "#/jobs/j-012", "#/jobs/12", "#/jobs/j-", "#/jobs/j-1%2F2", "#/jobs//j-1"]) assert.deepEqual(parse(bad), { view: "home" }, bad);
});

test("generated hashes: #/jobs/<id> opens a job only for one canonical job ID and no other part", () => {
  let seed = 11;
  const rand = (n) => ((seed = (seed * 1103515245 + 12345) % 2147483648), seed % n);
  const pieces = ["#/jobs", "/", "j-", "0", "1", "9", "x", "%", "%2F", "-", "jobs", "#"];
  let jobs = 0;
  for (let i = 0; i < 5000; i++) {
    const start = ["#/jobs/j-", "#/jobs/", "#jobs/j-", ""][rand(4)];
    const hash = start + Array.from({ length: rand(6) }, () => pieces[rand(pieces.length)]).join("");
    const route = parse(hash);
    if (route.view === "job") {
      const rest = hash.replace(/^#\/?/, "");
      assert.equal(rest, `jobs/${route.id}`, hash);
      assert.match(route.id, JOB_ID, hash);
      jobs++;
    }
    if (route.view === "jobs") assert.match(hash.replace(/^#\/?/, ""), /^jobs\/?$/, hash);
  }
  assert.ok(jobs > 0, "some generated hashes open a job");
  for (let n = 1; n < 2000; n += 7) assert.deepEqual(parse(href.job(`j-${n}`)), { view: "job", id: `j-${n}` });
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
  assert.equal(j.now(jobs["j-14"]), "It asks you a question.");
  assert.equal(j.now(jobs["j-12"]), "Waits for a reply, until Wed 18 Nov.");
  assert.equal(j.now({ ...jobs["j-12"], wait: { until: jobs["j-12"].wait.until } }), "Waits, until Wed 18 Nov.");
  assert.equal(j.now(jobs["j-9"]), "Done Fri 13 Nov.");
  assert.equal(j.now({ ...jobs["j-9"], end: "cancel" }), "Cancelled Fri 13 Nov.");
});

test("a working job with no running session says that no agent works on it, and how to start one (issue 742)", async () => {
  const d = fx.jobDetails["j-15"];
  const run = "iris job run --agent claude-code j-15";
  assert.deepEqual(j.agent(d), { said: "No agent works on it now.", ended: "The last session (se-3, claude-code) ended with exit code 1.", run });
  assert.deepEqual(j.agent({ ...d, session: null }), { said: "No agent works on it now.", run });
  const ended = (end, exit = null, state = "ended") => j.agent({ ...d, session: { ...d.session, state, end, exit } }).ended;
  assert.equal(ended("exit", 0), "The last session (se-3, claude-code) ended.");
  assert.equal(ended("launch_failed"), "The last session (se-3, claude-code) did not start.");
  assert.equal(ended("stop"), "The last session (se-3, claude-code) was stopped.");
  // An ending session is still live: the engine refuses a new one, so no start step.
  assert.deepEqual(j.agent({ ...d, session: { ...d.session, state: "ending" } }), { said: "No agent works on it now.", ended: "The last session (se-3, claude-code) stops now. When it has stopped, you can start an agent again." });
  // The start step names the agent of the last session.
  assert.equal(j.agent({ ...d, session: { ...d.session, agent: "codex" } }).run, "iris job run --agent codex j-15");
  for (const state of ["starting", "running"]) assert.equal(j.agent({ ...d, session: { ...d.session, state, end: null, exit: null } }), null, state);
  for (const id of ["j-12", "j-14"]) assert.equal(j.agent({ ...fx.jobDetails[id], session: null }), null, id);
  const page = text(await render("/src/views/Job.svelte", { detail: d }));
  for (const words of ["No agent works on it now.", "ended with exit code 1.", run]) assert.ok(page.includes(words), words);
  assert.ok(!page.includes("The agent works on it."), page);
});

test("a due date has a tone near its day and after it; an ended job keeps the date with no tone", () => {
  const job = { ...fx.jobs["j-12"], due: "2026-11-20T00:00:00.000Z" };
  assert.equal(j.due(job, new Date("2026-11-10T12:00:00Z")).tone, "");
  assert.equal(j.due(job, new Date("2026-11-18T12:00:00Z")).tone, "soon");
  assert.equal(j.due(job, new Date("2026-11-21T12:00:00Z")).tone, "late");
  for (const now of ["2026-11-10T12:00:00Z", "2026-11-21T12:00:00Z"]) {
    assert.deepEqual(j.due({ ...job, state: "done" }, new Date(now)), { text: "Was due Fri 20 Nov", tone: "" });
  }
  assert.equal(j.due(fx.jobs["j-15"]), null);
});

test("generated due dates: the words say late each time the tone is late", () => {
  const job = { ...fx.jobs["j-12"] };
  const base = Date.parse("2026-11-20T09:00:00Z");
  for (let m = -4 * 1440; m <= 4 * 1440; m += 37) {
    const now = new Date(base + m * 60_000);
    const d = j.due({ ...job, due: new Date(base).toISOString() }, now);
    assert.equal(d.tone === "late", d.text.startsWith("Was due"), `${now.toISOString()}: ${d.text}`);
    assert.equal(d.tone === "late", now.getTime() >= base);
  }
  // A deadline earlier on the same day.
  assert.match(j.due({ ...job, due: "2026-11-20T09:00:00Z" }, new Date("2026-11-20T10:00:00Z")).text, /^Was due .*earlier today$/);
});

test("a changeset whose newest revision is a draft gives no count and no review link", () => {
  for (const actions of [{}, { draft: 2 }, { draft: 1, replaced: 3 }]) {
    const d = structuredClone(fx.jobDetails["j-12"]);
    d.changesets[1] = { ...d.changesets[1], state: "draft", actions };
    const step = j.steps(d).find((s) => s.key === "cs-34");
    assert.equal(step.text, "The agent asked for changes. It now drafts a new version of the request.");
    assert.equal(step.link, undefined);
  }
});

test("a wait on a thread that is gone says so, with no link", () => {
  const d = structuredClone(fx.jobDetails["j-12"]);
  d.threads["t-8841"] = { id: "t-8841", gone: true };
  const wait = j.steps(d).find((s) => s.key === "e1");
  assert.equal(wait.mail, undefined);
  assert.match(wait.text, /The mail is gone\./);
});

test("a wait never names who must reply: the newest mail of the thread can be your own", async () => {
  const d = structuredClone(fx.jobDetails["j-12"]);
  d.threads["t-8841"] = { ...d.threads["t-8841"], from: { name: "You", address: "you@example.com" } };
  const wait = j.steps(d).find((s) => s.key === "e5");
  assert.deepEqual(wait.mail, { subject: "Re: Northwind contract, final version", link: "#/mail/m-90211" });
  const page = text(await render("/src/views/Job.svelte", { detail: d }));
  assert.ok(!page.includes("you@example.com") && !/from You/.test(page), page);
});

test("the job page shows the goal, the state and each step; agent text stays text", async () => {
  const d = structuredClone(fx.jobDetails["j-14"]);
  d.job.question = "<img src=x onerror=alert(1)> Pay it?";
  const html = await render("/src/views/Job.svelte", { detail: d });
  assert.ok(!html.includes("<img"), "the question is escaped");
  assert.ok(text(html).includes("Pay the Q4 invoice from Brightline."));
  assert.ok(text(html).includes("The agent asks you"));
  const page = text(await render("/src/views/Job.svelte", { detail: fx.jobDetails["j-12"] }));
  for (const words of ["Each step", "You gave the goal.", "Waits for a reply, until Wed 18 Nov.", "Re: Northwind contract, final version", "Accept 45 days."]) {
    assert.ok(page.includes(words), words);
  }
});

const pages = (over = {}) => ({ ...Object.fromEntries(["needs_you", "working", "waiting", "done"].map((st) => [st, fx.answer(`/jobs?state=${st}`)])), ...over });

test("the list puts the jobs that ask you first, and the ended jobs last", async () => {
  const html = text(await render("/src/views/Jobs.svelte", { pages: pages() }));
  const order = ["Asks you", "Pay the Q4 invoice", "Working", "Book a dentist", "Waiting", "Get the signed contract", "Ended", "Cancel the gym"].map((w) => html.indexOf(w));
  assert.ok(order.every((n) => n >= 0), String(order));
  assert.deepEqual(order, [...order].sort((a, b) => a - b));
  const none = { jobs: [], next: null };
  const empty = text(await render("/src/views/Jobs.svelte", { pages: { needs_you: none, working: none, waiting: none, done: none } }));
  assert.ok(empty.includes("No job yet."));
});

test("an old job that asks you shows first, also behind a full page of newer working jobs", async () => {
  const working = { jobs: Array.from({ length: 50 }, (_, i) => ({ ...fx.jobs["j-15"], id: `j-${1000 - i}`, goal: `Goal ${i}.` })), next: "j-951" };
  const html = text(await render("/src/views/Jobs.svelte", { pages: pages({ working }) }));
  assert.ok(html.indexOf("Pay the Q4 invoice") < html.indexOf("Goal 0."));
  assert.ok(html.includes("Show more"));
});

test("a job due date counts days in the browser, also when the owner zone differs (issue #892)", () => {
  // The day of the browser, as the start page picked it: the end of 17 Nov here.
  const end = new Date(2026, 10, 17, 23, 59);
  const job = { ...fx.jobs["j-12"], due: end.toISOString() };
  const now = new Date(2026, 10, 17, 0, 0);
  const browser = j.due(job, now).text;
  assert.match(browser, /today$/);
  const offset = -now.getTimezoneOffset() / 60;
  access.zone = offset >= 0 ? "Pacific/Kiritimati" : "Etc/GMT+12"; // A zone far from the browser.
  try {
    assert.equal(j.due(job, now).text, browser);
  } finally {
    access.zone = undefined;
  }
});
