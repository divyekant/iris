// Issue 322 in a DOM: the page of one job answers its question and ends the job.
process.env.TZ = "America/Los_Angeles";
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

const fx = await load("/dev/fixtures.ts");
const text = (el) => el.textContent.replace(/\s+/g, " ");
const button = (view, name) => [...view.target.querySelectorAll("button")].find((b) => text(b).trim() === name);

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
