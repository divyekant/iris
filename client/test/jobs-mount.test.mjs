// The Jobs view in a DOM: "Show more" asks for the same page size as the first
// request of each state (issue #329).
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

const fx = await load("/dev/fixtures.ts");
const { pageSize } = await load("/src/jobs.ts");

test("Show more asks for 20 ended jobs and 50 jobs of another state, the size of the first request", async () => {
  const asked = [];
  globalThis.fetch = async (url) => {
    asked.push(String(url));
    return new Response(JSON.stringify({ jobs: [], next: null }), { status: 200, headers: { "content-type": "application/json" } });
  };
  const page = (job) => ({ jobs: [job], next: "c1" });
  const pages = { needs_you: { jobs: [], next: null }, working: page(fx.jobs["j-15"]), waiting: { jobs: [], next: null }, done: page(fx.jobs["j-9"]) };
  const view = await mount("/src/views/Jobs.svelte", { pages });
  const buttons = () => [...view.target.querySelectorAll("button")].filter((b) => b.textContent.includes("Show more"));
  assert.equal(buttons().length, 2);
  buttons()[1].click();
  await until(() => asked.length === 1);
  assert.match(asked[0], /state=done&limit=20&after=c1$/);
  await until(() => buttons().length === 1);
  buttons()[0].click();
  await until(() => asked.length === 2);
  assert.match(asked[1], /state=working&limit=50&after=c1$/);
  assert.equal(pageSize("done"), 20);
  for (const state of ["needs_you", "working", "waiting"]) assert.equal(pageSize(state), 50);
  view.destroy();
});

test("a working job that no agent works on says so in the list, with how its last session ended (issue 791)", async () => {
  const running = { ...fx.jobs["j-15"], id: "j-80", goal: "Book the room." };
  const fresh = { ...fx.jobs["j-15"], id: "j-81", goal: "Pay the bill." };
  const failed = fx.jobs["j-15"];
  const working = {
    jobs: [running, fresh, failed],
    next: null,
    sessions: {
      "j-80": { id: "se-9", agent: "codex", state: "running", end: null, exit: null, ended: null },
      "j-81": null,
      "j-15": { id: "se-3", agent: "claude-code", state: "ended", end: "exit", exit: 1, ended: "2026-11-16T11:01:00Z" },
    },
  };
  const none = { jobs: [], next: null };
  const view = await mount("/src/views/Jobs.svelte", { pages: { needs_you: none, working, waiting: none, done: none } });
  try {
    const line = (id) => view.target.querySelector(`a[href="#/jobs/${id}"]`);
    assert.match(line("j-80").textContent, /The agent works on it\./);
    assert.equal(line("j-80").querySelector(".mark").classList.contains("idle"), false);
    assert.match(line("j-81").textContent, /No agent works on it now\./);
    assert.match(line("j-15").textContent, /No agent works on it now\. The last session \(se-3, claude-code\) ended with exit code 1\./);
    assert.equal(line("j-15").querySelector(".mark").classList.contains("idle"), true);
  } finally {
    view.destroy();
  }
  // An engine from before the field gives no sessions: the list does not guess.
  const old = await mount("/src/views/Jobs.svelte", { pages: { needs_you: none, working: { jobs: [failed], next: null }, waiting: none, done: none } });
  try {
    assert.match(old.target.querySelector('a[href="#/jobs/j-15"]').textContent, /The agent works on it\./);
  } finally {
    old.destroy();
  }
});

test("the brief names each working job that no agent works on, with a link to it (issue 791)", async () => {
  const failed = fx.jobs["j-15"];
  globalThis.fetch = async (url) => {
    const path = String(url).replace(/^\/api\//, "");
    const body = path.startsWith("jobs?state=working")
      ? { jobs: [failed, { ...failed, id: "j-80", goal: "Book the room." }], next: null, sessions: { "j-15": { id: "se-3", agent: "claude-code", state: "ended", end: "launch_failed", exit: null, ended: "2026-11-16T11:01:00Z" }, "j-80": { id: "se-9", agent: "codex", state: "starting", end: null, exit: null, ended: null } } }
      : fx.answer(`/${path}`);
    return new Response(JSON.stringify(body ?? {}), { status: 200, headers: { "content-type": "application/json" } });
  };
  const view = await mount("/src/views/Brief.svelte", { brief: fx.brief, watches: [], proposed: null, questions: [], now: new Date("2026-11-16T12:00:00Z"), poll: 0 });
  try {
    const part = await until(() => view.target.querySelector("#idle")?.closest("section"));
    const text = part.textContent.replace(/\s+/g, " ");
    assert.match(text, /No agent works on this job 1/);
    assert.match(text, /The last session \(se-3, claude-code\) did not start\./);
    assert.ok(part.querySelector('a[href="#/jobs/j-15"]'));
    assert.equal(part.querySelector('a[href="#/jobs/j-80"]'), null, "a session that starts is an agent at work");
  } finally {
    view.destroy();
  }
});

test("Show more adds the sessions of the new page and keeps the sessions of the first page (issue 791)", async () => {
  const first = { ...fx.jobs["j-15"], id: "j-80", goal: "Book the room." };
  const stopped = { ...fx.jobs["j-15"], id: "j-82", goal: "Ask for the quote." };
  const never = { ...fx.jobs["j-15"], id: "j-83", goal: "Pay the bill." };
  globalThis.fetch = async () =>
    new Response(
      JSON.stringify({
        jobs: [stopped, never],
        next: null,
        sessions: { "j-82": { id: "se-5", agent: "codex", state: "ended", end: "exit", exit: 2, ended: "2026-11-16T11:01:00Z" }, "j-83": null },
      }),
      { status: 200, headers: { "content-type": "application/json" } },
    );
  const none = { jobs: [], next: null };
  const working = { jobs: [first], next: "c1", sessions: { "j-80": { id: "se-9", agent: "codex", state: "running", end: null, exit: null, ended: null } } };
  const view = await mount("/src/views/Jobs.svelte", { pages: { needs_you: none, working, waiting: none, done: none } });
  try {
    const line = (id) => view.target.querySelector(`a[href="#/jobs/${id}"]`);
    [...view.target.querySelectorAll("button")].find((b) => b.textContent.includes("Show more")).click();
    await until(() => line("j-83"));
    assert.match(line("j-82").textContent, /No agent works on it now\. The last session \(se-5, codex\) ended with exit code 2\./);
    assert.equal(line("j-82").querySelector(".mark").classList.contains("idle"), true);
    assert.match(line("j-83").textContent, /No agent works on it now\./);
    assert.equal(line("j-83").querySelector(".mark").classList.contains("idle"), true);
    assert.match(line("j-80").textContent, /The agent works on it\./);
    assert.equal(line("j-80").querySelector(".mark").classList.contains("idle"), false);
  } finally {
    view.destroy();
  }
});
