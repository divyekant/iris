// Issue 322 in a DOM: the page of one job answers its question and ends the job.
process.env.TZ = "America/Los_Angeles";
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

const fx = await load("/dev/fixtures.ts");
const text = (el) => el.textContent.replace(/\s+/g, " ");
const button = (view, name) => [...view.target.querySelectorAll("button")].find((b) => text(b).trim() === name);

// The engine adds one event for each move of a job, as an end does.
const doneOf = (d, kind = "complete") => ({
  ...d,
  job: { ...d.job, state: "done", end: kind, wait: undefined, question: undefined, events: [...d.job.events, { at: "2026-11-16T20:00:00Z", state: "done", by: "person", end: kind }] },
});

function serve(answer) {
  const asked = [];
  globalThis.fetch = async (url, init) => {
    const path = String(url).replace(/^\/api\//, "");
    const method = init?.method ?? "GET";
    const body = init?.body ? JSON.parse(init.body) : undefined;
    if (path === "policy") return new Response(JSON.stringify(fx.answer("/policy")), { status: 200, headers: { "content-type": "application/json" } });
    asked.push([method, path, body]);
    const out = await answer(method, path, body);
    if (out instanceof Error) throw out;
    return new Response(JSON.stringify(out[1]), { status: out[0], headers: { "content-type": "application/json" } });
  };
  return asked;
}

test("a job that asks you takes the answer on its page, and shows the job that goes on", async () => {
  const d = structuredClone(fx.jobDetails["j-14"]);
  const on = { ...d, job: { ...d.job, state: "working", question: undefined } };
  const asked = serve((method) => (method === "POST" ? [200, on] : [200, on]));
  const view = await mount("/src/views/Job.svelte", { detail: d, every: 60_000 });
  try {
    const now = text(view.target.querySelector("[aria-label=Now]"));
    assert.match(now, /The agent asks you/);
    assert.ok(now.includes(d.job.question));
    const field = view.target.querySelector("[aria-label=Now] textarea");
    field.value = "Use the amount of the invoice.";
    field.dispatchEvent(new window.Event("input", { bubbles: true }));
    view.flushSync();
    view.target.querySelector("[aria-label=Now] form").dispatchEvent(new window.Event("submit", { bubbles: true, cancelable: true }));
    await until(() => !view.target.querySelector("[aria-label=Now] textarea"));
    assert.deepEqual(asked[0], ["POST", "jobs/j-14/answer", { answer: "Use the amount of the invoice.", question: d.job.question, updated: d.job.updated }]);
  } finally {
    view.destroy();
  }
});

test("End asks in place: Keep the job changes nothing, and Cancel the job ends it", async () => {
  const d = structuredClone(fx.jobDetails["j-12"]);
  const ended = { ...d, job: { ...d.job, state: "done", end: "cancel", wait: undefined } };
  const asked = serve((method) => (method === "POST" ? [200, ended] : [200, d]));
  const view = await mount("/src/views/Job.svelte", { detail: d, every: 60_000 });
  try {
    button(view, "End").click();
    view.flushSync();
    assert.match(text(view.target.querySelector("[role=group]")), /End this job\?/);
    button(view, "Keep the job").click();
    view.flushSync();
    assert.equal(view.target.querySelector("[role=group]"), null);
    assert.equal(asked.length, 0);
    button(view, "End").click();
    view.flushSync();
    button(view, "Cancel the job").click();
    await until(() => !button(view, "End"));
    assert.deepEqual(asked[0], ["POST", "jobs/j-12/finish", { end: "cancel" }]);
    assert.match(text(view.target.querySelector("[aria-label=Now]")), /Cancelled/);
    assert.ok(!button(view, "Edit"));
  } finally {
    view.destroy();
  }
});

test("Mark as done sends complete; a lost answer reads the job again and says what it knows", async () => {
  const d = structuredClone(fx.jobDetails["j-12"]);
  const done = { ...d, job: { ...d.job, state: "done", end: "complete", wait: undefined } };
  // The answer is lost, and the read after it shows the job done: the page shows it.
  let read = done;
  let asked = serve((method) => (method === "POST" ? new TypeError("Failed to fetch") : [200, read]));
  let view = await mount("/src/views/Job.svelte", { detail: d, every: 60_000 });
  try {
    button(view, "End").click();
    view.flushSync();
    button(view, "Mark as done").click();
    await until(() => !button(view, "End"));
    assert.deepEqual(asked[0], ["POST", "jobs/j-12/finish", { end: "complete" }]);
    assert.deepEqual(asked[1].slice(0, 2), ["GET", "jobs/j-12"]);
    assert.match(text(view.target.querySelector("[aria-label=Now]")), /Done/);
  } finally {
    view.destroy();
  }
  // The read shows the job still open: the page does not say that it ended.
  read = d;
  asked = serve((method) => (method === "POST" ? new TypeError("Failed to fetch") : [200, read]));
  view = await mount("/src/views/Job.svelte", { detail: d, every: 60_000 });
  try {
    button(view, "End").click();
    view.flushSync();
    button(view, "Mark as done").click();
    await until(() => view.target.querySelector("[role=alert]"));
    assert.match(text(view.target.querySelector("[role=alert]")), /Iris does not know if the job ended/);
    assert.ok(button(view, "Mark as done"), "the question stays, so the person can try again");
  } finally {
    view.destroy();
  }
});

test("a done job has no End", async () => {
  const d = structuredClone(fx.jobDetails["j-12"]);
  serve(() => [200, d]);
  const view = await mount("/src/views/Job.svelte", { detail: { ...d, job: { ...d.job, state: "done", end: "complete" } }, every: 60_000 });
  assert.ok(!button(view, "End"));
  view.destroy();
});

test("an answer or an end whose job document is lost or fails reads the job again (pre-review)", async () => {
  // The answer commits, but its body does not parse: the page reads the job and shows it.
  const d = structuredClone(fx.jobDetails["j-14"]);
  const on = { ...d, job: { ...d.job, state: "working", question: undefined } };
  globalThis.fetch = async (url, init) => {
    const path = String(url).replace(/^\/api\//, "");
    if (path === "policy") return new Response(JSON.stringify(fx.answer("/policy")), { status: 200, headers: { "content-type": "application/json" } });
    if (init?.method === "POST") return new Response("{\"job\":", { status: 200, headers: { "content-type": "application/json" } });
    return new Response(JSON.stringify(on), { status: 200, headers: { "content-type": "application/json" } });
  };
  let view = await mount("/src/views/Job.svelte", { detail: d, every: 60_000 });
  try {
    const field = view.target.querySelector("[aria-label=Now] textarea");
    field.value = "Yes.";
    field.dispatchEvent(new window.Event("input", { bubbles: true }));
    view.flushSync();
    view.target.querySelector("[aria-label=Now] form").dispatchEvent(new window.Event("submit", { bubbles: true, cancelable: true }));
    await until(() => !view.target.querySelector("[aria-label=Now] textarea"));
    assert.match(text(view.target.querySelector("[aria-label=Now]")), /works on it|No agent/);
  } finally {
    view.destroy();
  }
  // The end commits, then the engine fails (500): the page reads the job and shows it done.
  const w = structuredClone(fx.jobDetails["j-12"]);
  const done = { ...w, job: { ...w.job, state: "done", end: "complete", wait: undefined } };
  for (const answer of [[500, { error: "The engine answered 500." }], [200, null]]) {
    const asked = serve((method) => (method === "POST" ? answer : [200, done]));
    view = await mount("/src/views/Job.svelte", { detail: w, every: 60_000 });
    try {
      button(view, "End").click();
      view.flushSync();
      button(view, "Mark as done").click();
      await until(() => !button(view, "End"));
      assert.deepEqual(asked[1].slice(0, 2), ["GET", "jobs/j-12"]);
      assert.match(text(view.target.querySelector("[aria-label=Now]")), /Done/);
    } finally {
      view.destroy();
    }
  }
});

test("a late answer response does not put back a job that a newer end replaced (pre-review)", async () => {
  const d = structuredClone(fx.jobDetails["j-14"]);
  // The engine adds an event for each move: the answer first, then the end.
  const on = { ...d, job: { ...d.job, state: "working", question: undefined, events: [...d.job.events, { at: "2026-11-16T19:00:00Z", state: "working", by: "person", answer: "Yes." }] } };
  const done = doneOf(on, "cancel");
  let release;
  const held = new Promise((r) => (release = r));
  serve(async (method, path) => {
    if (path === "jobs/j-14/answer") return await held, [200, on];
    if (path === "jobs/j-14/finish") return [200, done];
    return [200, done];
  });
  const view = await mount("/src/views/Job.svelte", { detail: d, every: 60_000 });
  try {
    const field = view.target.querySelector("[aria-label=Now] textarea");
    field.value = "Yes.";
    field.dispatchEvent(new window.Event("input", { bubbles: true }));
    view.flushSync();
    view.target.querySelector("[aria-label=Now] form").dispatchEvent(new window.Event("submit", { bubbles: true, cancelable: true }));
    button(view, "End").click();
    view.flushSync();
    button(view, "Cancel the job").click();
    await until(() => !button(view, "End"));
    release();
    await new Promise((r) => setTimeout(r, 20));
    view.flushSync();
    assert.match(text(view.target.querySelector("[aria-label=Now]")), /Cancelled/);
    assert.ok(!button(view, "End"), "the job stays done");
  } finally {
    view.destroy();
  }
});


test("a poll that read the job before the end commits does not hide the end (review round 2)", async () => {
  const d = structuredClone(fx.jobDetails["j-15"]);
  let release;
  const finished = new Promise((r) => (release = r));
  let reads = 0;
  serve(async (method, path) => {
    if (method === "POST" && path === "jobs/j-15/finish") return (await finished, [200, doneOf(d)]);
    // A poll that starts after the end request reads the job from before the end.
    reads++;
    return [200, d];
  });
  const view = await mount("/src/views/Job.svelte", { detail: d, every: 20 });
  try {
    button(view, "End").click();
    view.flushSync();
    button(view, "Mark as done").click();
    const before = reads;
    await until(() => reads > before);
    await new Promise((r) => setTimeout(r, 30));
    release();
    await until(() => (view.flushSync(), !button(view, "End")));
    assert.doesNotMatch(text(view.target), /does not know if the job ended/);
    assert.equal(view.target.querySelector("[aria-label='End the job']"), null);
  } finally {
    view.destroy();
  }
});

test("a late scope answer does not put back a job that ended after it (review round 2)", async () => {
  const d = structuredClone(fx.jobDetails["j-12"]);
  let release;
  const removed = new Promise((r) => (release = r));
  const posts = [];
  serve(async (method, path) => {
    if (method === "POST") posts.push(path);
    if (method === "POST" && path === "jobs/j-12/scope/remove") return (await removed, [200, { ...d, scope: [] }]);
    if (method === "POST" && path === "jobs/j-12/finish") return [200, doneOf(d, "cancel")];
    return [200, d];
  });
  const view = await mount("/src/views/Job.svelte", { detail: d, every: 60_000 });
  try {
    button(view, "Remove").click();
    await until(() => posts.includes("jobs/j-12/scope/remove"));
    button(view, "End").click();
    view.flushSync();
    button(view, "Cancel the job").click();
    await until(() => (view.flushSync(), !button(view, "End")));
    release();
    await new Promise((r) => setTimeout(r, 30));
    view.flushSync();
    assert.equal(button(view, "End"), undefined, "the job stays done");
  } finally {
    view.destroy();
  }
});

test("a poll that read the job before an edit commits does not undo the edit (review round 3)", async () => {
  const d = structuredClone(fx.jobDetails["j-15"]);
  const edited = { ...d, job: { ...d.job, goal: "Book the dentist for Tuesday morning." } };
  let committed = false;
  let release;
  const stale = new Promise((r) => (release = r));
  let polls = 0;
  serve(async (method, path) => {
    if (method === "POST" && path === "jobs/j-15/edit") {
      // A poll starts while the edit runs, and reads the job from before it.
      await until(() => polls > 0);
      committed = true;
      return [200, edited.job];
    }
    if (!committed) {
      polls++;
      await stale;
      return [200, d];
    }
    return [200, edited];
  });
  const view = await mount("/src/views/Job.svelte", { detail: d, every: 20 });
  try {
    button(view, "Edit").click();
    view.flushSync();
    const goal = view.target.querySelector(".job-form textarea");
    goal.value = edited.job.goal;
    goal.dispatchEvent(new window.Event("input", { bubbles: true }));
    view.flushSync();
    view.target.querySelector(".job-form").dispatchEvent(new window.Event("submit", { bubbles: true, cancelable: true }));
    await until(() => (view.flushSync(), !view.target.querySelector(".job-form")));
    release();
    await new Promise((r) => setTimeout(r, 60));
    view.flushSync();
    assert.equal(text(view.target.querySelector("h1")).trim(), edited.job.goal);
  } finally {
    view.destroy();
  }
});

test("Edit closes the end question, so a done job never keeps an open edit (review round 3)", async () => {
  const d = structuredClone(fx.jobDetails["j-12"]);
  serve(() => [200, d]);
  const view = await mount("/src/views/Job.svelte", { detail: d, every: 60_000 });
  try {
    button(view, "End").click();
    view.flushSync();
    assert.ok(button(view, "Mark as done"));
    button(view, "Edit").click();
    view.flushSync();
    assert.equal(button(view, "Mark as done"), undefined);
    assert.ok(view.target.querySelector(".job-form"));
  } finally {
    view.destroy();
  }
});
