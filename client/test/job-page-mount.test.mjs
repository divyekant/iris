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
    // The end waits for the answer: the page runs one job request at a time.
    release();
    await until(() => (view.flushSync(), !button(view, "End")));
    await new Promise((r) => setTimeout(r, 20));
    view.flushSync();
    assert.match(text(view.target.querySelector("[aria-label=Now]")), /Cancelled/);
    assert.ok(!button(view, "End"), "the job stays done");
  } finally {
    view.destroy();
  }
});


// The page runs its job requests one at a time (review round 4): each case holds the first request,
// starts a second one, and checks that the second waits, and that the page ends on the job of the last.
function queueServe(first, second) {
  let release;
  const held = new Promise((r) => (release = r));
  const log = { open: 0, most: 0, order: [] };
  globalThis.fetch = async (url, init) => {
    const path = String(url).replace(/^\/api\//, "");
    const method = init?.method ?? "GET";
    if (path === "policy") return new Response(JSON.stringify(fx.answer("/policy")), { status: 200, headers: { "content-type": "application/json" } });
    log.open++;
    log.most = Math.max(log.most, log.open);
    log.order.push(`${method} ${path}`);
    try {
      const out = first.match(method, path) && !first.done ? ((first.done = true), await held, first.answer) : second(method, path);
      return new Response(JSON.stringify(out), { status: 200, headers: { "content-type": "application/json" } });
    } finally {
      log.open--;
    }
  };
  return { log, release };
}
const press = (view, name) => (button(view, name).click(), view.flushSync());
function editTo(view, goal) {
  press(view, "Edit");
  const field = view.target.querySelector(".job-form textarea");
  field.value = goal;
  field.dispatchEvent(new window.Event("input", { bubbles: true }));
  view.flushSync();
  view.target.querySelector(".job-form").dispatchEvent(new window.Event("submit", { bubbles: true, cancelable: true }));
}

test("the job requests of the page run one at a time, and the page shows the job of the last (review round 4)", async () => {
  const j15 = structuredClone(fx.jobDetails["j-15"]);
  const j12 = structuredClone(fx.jobDetails["j-12"]);
  const goal = "Book the dentist for Tuesday morning.";
  const cases = [
    // A poll reads the job before an end: the end waits for it, then shows.
    { name: "poll, then end", d: j15, every: 20, first: { match: (m) => m === "GET", answer: j15 }, act: (v) => (press(v, "End"), press(v, "Mark as done")),
      second: (m, p) => (p.endsWith("/finish") ? doneOf(j15) : doneOf(j15)), check: (v) => !button(v, "End") },
    // A poll reads the job before an edit: the edit waits for it, then shows.
    { name: "poll, then edit", d: j15, every: 20, first: { match: (m) => m === "GET", answer: j15 }, act: (v) => editTo(v, goal),
      second: (m) => (m === "POST" ? { ...j15.job, goal } : { ...j15, job: { ...j15.job, goal } }), check: (v) => v.target.querySelector("h1")?.textContent.trim() === goal },
    // A scope change, then an end.
    { name: "scope, then end", d: j12, every: 60_000, first: { match: (m, p) => p.endsWith("/scope/remove"), answer: { ...j12, scope: [] } }, act: (v) => (press(v, "End"), press(v, "Cancel the job")),
      second: () => doneOf({ ...j12, scope: [] }, "cancel"), check: (v) => !button(v, "End") },
    // A scope change, then a goal edit (an edit adds no event).
    { name: "scope, then edit", d: j12, every: 60_000, first: { match: (m, p) => p.endsWith("/scope/remove"), answer: { ...j12, scope: [] } }, act: (v) => editTo(v, goal),
      second: (m) => (m === "POST" ? { ...j12.job, goal } : { ...j12, scope: [], job: { ...j12.job, goal } }), check: (v) => v.target.querySelector("h1")?.textContent.trim() === goal },
  ];
  for (const c of cases) {
    const { log, release } = queueServe(c.first, c.second);
    const view = await mount("/src/views/Job.svelte", { detail: structuredClone(c.d), every: c.every });
    try {
      if (c.d === j12) press(view, "Remove");
      await until(() => c.first.done);
      c.act(view);
      await new Promise((r) => setTimeout(r, 40));
      assert.equal(log.open, 1, `${c.name}: the second request waits`);
      release();
      await until(() => (view.flushSync(), c.check(view)));
      await new Promise((r) => setTimeout(r, 40));
      view.flushSync();
      assert.ok(c.check(view), `${c.name}: the page ends on the job of the last request`);
      assert.equal(log.most, 1, `${c.name}: one request at a time`);
    } finally {
      view.destroy();
    }
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

// Issue 959: the person ends the wait of a job early.
test("Wake now ends the wait of a waiting job, with the updated of the job that the person saw", async () => {
  const d = structuredClone(Object.values(fx.jobDetails).find((x) => x.job.state === "waiting"));
  const woke = { ...d, job: { ...d.job, state: "working", wait: undefined, events: [...d.job.events, { at: "2026-11-16T20:00:00Z", state: "working", by: "person" }] } };
  const asked = serve((method) => (method === "POST" ? [200, woke] : [200, woke]));
  const view = await mount("/src/views/Job.svelte", { detail: d, every: 60_000 });
  try {
    button(view, "Wake now").click();
    await until(() => (view.flushSync(), !button(view, "Wake now")));
    assert.deepEqual(asked[0], ["POST", `jobs/${d.job.id}/wake`, { updated: d.job.updated }]);
    assert.match(text(view.target), /You ended the wait\./);
  } finally {
    view.destroy();
  }
});

test("a refused wake reads the job again and says why; a job that does not wait has no Wake now", async () => {
  const d = structuredClone(Object.values(fx.jobDetails).find((x) => x.job.state === "waiting"));
  const moved = { ...d, job: { ...d.job, wait: { ...d.job.wait, until: "2026-12-01T00:00:00Z" }, updated: "2026-11-16T21:00:00.000Z" } };
  const asked = serve((method) => (method === "POST" ? [409, { error: "The job does not wait now, or its wait changed." }] : [200, moved]));
  const view = await mount("/src/views/Job.svelte", { detail: d, every: 60_000 });
  try {
    button(view, "Wake now").click();
    await until(() => (view.flushSync(), view.target.querySelector("[aria-label=Now] [role=alert]")));
    assert.match(text(view.target.querySelector("[aria-label=Now] [role=alert]")), /its wait changed/);
    assert.deepEqual(asked.map(([m]) => m), ["POST", "GET"]);
    assert.ok(button(view, "Wake now"), "the new wait can still be woken");
  } finally {
    view.destroy();
  }
  for (const x of Object.values(fx.jobDetails).filter((x) => x.job.state !== "waiting")) {
    const v = await mount("/src/views/Job.svelte", { detail: structuredClone(x), every: 60_000 });
    assert.ok(!button(v, "Wake now"), x.job.id);
    v.destroy();
  }
});
