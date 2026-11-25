// Decision D25 in a DOM: a new job from a typed goal, an edit of the goal and
// the due date, and the scope of a job (remove, search, preview, add). And
// D24: the job page names Iris as the asker of the due-date question.
process.env.TZ = "America/Los_Angeles";
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

globalThis.location = window.location;
Object.defineProperty(globalThis, "sessionStorage", { configurable: true, value: window.sessionStorage });
const fx = await load("/dev/fixtures.ts");
const s = await load("/src/start.ts");
const text = (el) => el.textContent.replace(/\s+/g, " ");
const button = (view, name) => [...view.target.querySelectorAll("button")].find((b) => text(b).trim() === name);
const empty = { jobs: [], next: null };

function serve(answer) {
  const asked = [];
  globalThis.fetch = async (url, init) => {
    const path = String(url).replace(/^\/api\//, "");
    const method = init?.method ?? "GET";
    const body = init?.body ? JSON.parse(init.body) : undefined;
    // The grants of the job page read the policy: the fixture answers it.
    if (path === "policy") return new Response(JSON.stringify(fx.answer("/policy")), { status: 200, headers: { "content-type": "application/json" } });
    asked.push([method, path, body]);
    const [status, out] = await answer(method, path, body);
    return new Response(JSON.stringify(out), { status, headers: { "content-type": "application/json" } });
  };
  return asked;
}
const fill = (view, selector, value) => {
  const el = view.target.querySelector(selector);
  el.value = value;
  el.dispatchEvent(new window.Event("input", { bubbles: true }));
  view.flushSync();
};
const submit = (view, selector = "form") => {
  view.target.querySelector(selector).dispatchEvent(new window.Event("submit", { bubbles: true, cancelable: true }));
  view.flushSync();
};

test("keepDue keeps the instant of an unchanged day, and threadsOf gives one line for each thread", () => {
  assert.equal(s.keepDue(undefined, ""), null);
  assert.equal(s.keepDue("2026-11-20T08:00:00.000Z", ""), null);
  assert.equal(s.keepDue("2026-11-20T08:00:00.000Z", "2026-11-20"), "2026-11-20T08:00:00.000Z");
  assert.equal(s.keepDue("2026-11-20T08:00:00.000Z", "2026-11-21"), s.endOfDay("2026-11-21"));
  assert.equal(s.keepDue(undefined, "2026-11-21"), s.endOfDay("2026-11-21"));
  const m = (id, thread, subject) => ({ id, thread, subject, date: null, from: {}, labels: [] });
  assert.deepEqual(s.threadsOf([m("m-1", "t-1", "A"), m("m-2", "t-2", " "), m("m-3", "t-1", "B")]), [
    { thread: "t-1", subject: "A", hits: 2 },
    { thread: "t-2", subject: "(no subject)", hits: 1 },
  ]);
});

test("New job sends the typed goal with one request ID, and a retry uses the same ID", async () => {
  let fail = true;
  const asked = serve(() => (fail ? ((fail = false), [500, { error: "The engine answered 500." }]) : [201, { ...fx.jobs["j-15"], id: "j-30" }]));
  const view = await mount("/src/views/Jobs.svelte", { pages: { needs_you: empty, working: empty, waiting: empty, done: empty } });
  button(view, "New job").click();
  view.flushSync();
  assert.ok(!button(view, "New job"));
  fill(view, "textarea", "  Book a dentist appointment.  ");
  submit(view);
  await until(() => view.target.querySelector("[role=alert]"));
  submit(view);
  await until(() => location.hash === "#/jobs/j-30");
  assert.equal(asked.length, 2);
  for (const [method, path, body] of asked) {
    assert.equal(method, "POST");
    assert.equal(path, "jobs");
    assert.deepEqual(Object.keys(body).sort(), ["due", "goal", "request"]);
    assert.equal(body.goal, "Book a dentist appointment.");
    assert.equal(body.due, null);
  }
  assert.ok(asked[0][2].request.length > 0 && asked[0][2].request === asked[1][2].request);
  view.destroy();
});

test("a start that the engine has already gives 409 and opens that job", async () => {
  serve(() => [409, { error: "This start made a job already.", job: "j-31" }]);
  const view = await mount("/src/views/Jobs.svelte", { pages: { needs_you: empty, working: empty, waiting: empty, done: empty } });
  button(view, "New job").click();
  view.flushSync();
  fill(view, "textarea", "Goal");
  submit(view);
  await until(() => location.hash === "#/jobs/j-31");
  view.destroy();
});

test("Edit saves the goal, keeps the due instant of an unchanged day, and shows the job again", async () => {
  const d = structuredClone(fx.jobDetails["j-12"]);
  const asked = serve((method) => (method === "POST" ? [200, { ...d.job, goal: "New goal" }] : [200, { ...d, job: { ...d.job, goal: "New goal" } }]));
  const view = await mount("/src/views/Job.svelte", { detail: d });
  button(view, "Edit").click();
  view.flushSync();
  assert.equal(view.target.querySelector("textarea").value, d.job.goal);
  fill(view, "textarea", "New goal");
  submit(view, "form.job-form");
  await until(() => view.target.querySelector("h1")?.textContent === "New goal");
  assert.deepEqual(asked[0], ["POST", "jobs/j-12/edit", { goal: "New goal", due: d.job.due }]);
  assert.deepEqual(asked[1].slice(0, 2), ["GET", "jobs/j-12"]);
  view.destroy();
  // A done job has no Edit, no Remove and no Add mail.
  const done = await mount("/src/views/Job.svelte", { detail: { ...d, job: { ...d.job, state: "done", end: "complete" } } });
  for (const name of ["Edit", "Remove", "Add mail"]) assert.ok(!button(done, name), name);
  done.destroy();
});

test("the scope shows each ref; Remove and Add send the ref and the IDs of the preview", async () => {
  const d = structuredClone(fx.jobDetails["j-12"]);
  const preview = { messages: [{ ...fx.answer("/items/c-311/job").messages[0] }, { ...fx.answer("/items/c-311/job").messages[2] }] };
  let changed = true;
  const asked = serve((method, path) => {
    if (path.startsWith("search")) return [200, { messages: preview.messages }];
    if (path.startsWith("jobs/j-12/scope/") && method === "GET") return [200, preview];
    if (path === "jobs/j-12/scope" && changed) return (changed = false), [409, { error: "The mail changed. Look at it again." }];
    if (path === "jobs/j-12/scope") return [200, { ...d, scope: [...d.scope, { id: "t-1", kind: "thread", label: "Q4", messages: 1 }] }];
    if (path === "jobs/j-12/scope/remove") return [200, { ...d, scope: [] }];
    return [404, {}];
  });
  const view = await mount("/src/views/Job.svelte", { detail: d });
  assert.match(text(view.target), /Mail the agent reads Northwind contract, final version 4 messages/);
  button(view, "Add mail").click();
  view.flushSync();
  fill(view, "input[type=search]", "Q4 numbers");
  submit(view, "form.search");
  const hit = await until(() => view.target.querySelector("button.hit"));
  assert.equal(asked.at(-1)[1], "search?q=Q4%20numbers&limit=50");
  hit.click();
  await until(() => button(view, "Add this thread"));
  assert.match(text(view.target), /Hidden from the agent for now/);
  button(view, "Add this thread").click();
  // The mail changed: the engine refuses, and the preview loads again with the note.
  await until(() => view.target.querySelector("[role=alert]") && asked.filter(([m, p]) => m === "GET" && p.includes("/scope/")).length === 2);
  assert.match(text(view.target), /The mail changed/);
  button(view, "Add this thread").click();
  await until(() => !button(view, "Add this thread"));
  const adds = asked.filter(([m, p]) => m === "POST" && p === "jobs/j-12/scope");
  assert.equal(adds.length, 2);
  assert.deepEqual(adds[1][2], { thread: preview.messages[0].thread, messages: preview.messages.map((m) => m.id) });
  assert.match(text(view.target), /Q4 1 message/);
  button(view, "Remove").click();
  await until(() => /No mail\./.test(text(view.target)));
  assert.deepEqual(asked.at(-1), ["POST", "jobs/j-12/scope/remove", { ref: "t-8841" }]);
  view.destroy();
});

test("a lost answer of a start keeps the draft and its request ID across a reload", async () => {
  window.sessionStorage.clear();
  // The first start reaches the engine, but its answer is lost.
  let lost = true;
  const asked = [];
  globalThis.fetch = async (url, init) => {
    asked.push(JSON.parse(init.body));
    if (lost) {
      lost = false;
      throw new TypeError("Failed to fetch");
    }
    return new Response(JSON.stringify({ error: "This start made a job already.", job: "j-40" }), { status: 409, headers: { "content-type": "application/json" } });
  };
  const pages = { needs_you: empty, working: empty, waiting: empty, done: empty };
  const first = await mount("/src/views/Jobs.svelte", { pages });
  button(first, "New job").click();
  first.flushSync();
  fill(first, "textarea", "Renew the passport.");
  submit(first);
  await until(() => first.target.querySelector("[role=alert]"));
  first.destroy();
  // The reload shows the form with the draft, and the retry sends the same ID.
  const again = await mount("/src/views/Jobs.svelte", { pages });
  assert.equal(again.target.querySelector("textarea")?.value, "Renew the passport.");
  submit(again);
  await until(() => location.hash === "#/jobs/j-40");
  assert.equal(asked.length, 2);
  assert.equal(asked[1].request, asked[0].request);
  assert.equal(globalThis.sessionStorage.getItem("iris.newJob"), null);
  again.destroy();
});

test("an edit or a remove with no answer shows the job as the engine has it", async () => {
  const d = structuredClone(fx.jobDetails["j-12"]);
  const saved = { ...d, job: { ...d.job, goal: "New goal" } };
  // The edit arrives; the read after it fails once, then the page reads the job again.
  let reads = 0;
  serve((method) => (method === "POST" ? [200, saved.job] : ++reads === 1 ? [500, { error: "The engine answered 500." }] : [200, saved]));
  const view = await mount("/src/views/Job.svelte", { detail: d });
  button(view, "Edit").click();
  view.flushSync();
  fill(view, "textarea", "New goal");
  submit(view, "form.job-form");
  await until(() => view.target.querySelector("h1")?.textContent === "New goal");
  assert.equal(view.target.querySelector("[role=alert]"), null);
  view.destroy();
  // The remove arrives, but its answer is lost: the ref goes, with no error.
  const gone = { ...d, scope: [] };
  globalThis.fetch = async (url, init) => {
    if ((init?.method ?? "GET") === "POST") throw new TypeError("Failed to fetch");
    const path = String(url).replace(/^\/api\//, "");
    const out = path === "policy" ? fx.answer("/policy") : gone;
    return new Response(JSON.stringify(out), { status: 200, headers: { "content-type": "application/json" } });
  };
  const page = await mount("/src/views/Job.svelte", { detail: d });
  button(page, "Remove").click();
  await until(() => /No mail\./.test(text(page.target)));
  assert.equal(page.target.querySelector(".refs"), null);
  assert.equal(page.target.querySelector("[role=alert]"), null);
  page.destroy();
});

test("a refused add of a thread in the scope already keeps the error and shows the thread again", async () => {
  const d = structuredClone(fx.jobDetails["j-12"]);
  const ref = d.scope.find((r) => r.kind === "thread");
  const preview = { messages: [{ ...fx.answer("/items/c-311/job").messages[0], thread: ref.id }] };
  const asked = serve((method, path) => {
    if (path.startsWith("search")) return [200, { messages: preview.messages }];
    if (path.startsWith("jobs/j-12/scope/") && method === "GET") return [200, preview];
    if (path === "jobs/j-12/scope") return [409, { error: "The mail changed. Look at it again." }];
    if (path === "jobs/j-12") return [200, d];
    return [404, {}];
  });
  const view = await mount("/src/views/Job.svelte", { detail: d });
  button(view, "Add mail").click();
  view.flushSync();
  fill(view, "input[type=search]", "contract");
  submit(view, "form.search");
  (await until(() => view.target.querySelector("button.hit"))).click();
  await until(() => button(view, "Add this thread"));
  button(view, "Add this thread").click();
  await until(() => asked.filter(([m, p]) => m === "GET" && p.includes("/scope/")).length === 2);
  await until(() => button(view, "Add this thread"));
  assert.match(text(view.target), /The mail changed/);
  view.destroy();
});

test("a search that fills its page offers more results, up to the limit of the API", async () => {
  const d = structuredClone(fx.jobDetails["j-12"]);
  const m = (i) => ({ id: `m-${i}`, thread: i < 60 ? "t-1" : `t-${i}`, subject: `S${i}`, date: null, from: {}, labels: [] });
  const asked = serve((method, path) => {
    const n = Number(new URLSearchParams(path.split("?")[1]).get("limit"));
    return [200, { messages: Array.from({ length: n }, (_, i) => m(i)) }];
  });
  const view = await mount("/src/views/Job.svelte", { detail: d });
  button(view, "Add mail").click();
  view.flushSync();
  fill(view, "input[type=search]", "invoice");
  submit(view, "form.search");
  // The first 50 matches are all of one thread: More results reaches the others.
  await until(() => button(view, "More results"));
  assert.equal(view.target.querySelectorAll("button.hit").length, 1);
  button(view, "More results").click();
  await until(() => view.target.querySelectorAll("button.hit").length > 1);
  assert.equal(asked.at(-1)[1], "search?q=invoice&limit=150");
  button(view, "More results").click();
  await until(() => /newest 200 matches/.test(text(view.target)));
  assert.equal(asked.at(-1)[1], "search?q=invoice&limit=200");
  assert.ok(!button(view, "More results"));
  view.destroy();
});

test("a due-date question names Iris as the asker, an agent question names the agent (issue #869)", async () => {
  serve(() => [200, {}]);
  const due = await mount("/src/views/Job.svelte", { detail: fx.jobDetails["j-16"] });
  assert.match(text(due.target), /Iris asks you/);
  assert.match(text(due.target), /Iris asked you: The due date passed\./);
  due.destroy();
  const agent = await mount("/src/views/Job.svelte", { detail: fx.jobDetails["j-14"] });
  assert.match(text(agent.target), /The agent asks you/);
  assert.match(text(agent.target), /The agent asked you:/);
  agent.destroy();
});

test("the due field of an edit follows the owner zone that comes after the form opens (owner time zone, D1)", async () => {
  const { access } = await load("/src/access.svelte.ts");
  const keep = access.zone;
  access.zone = undefined;
  const old = "2026-11-20T17:00:00Z";
  const sent = [];
  const view = await mount("/src/views/JobForm.svelte", {
    goal: "Book the dentist",
    due: old,
    action: "Save",
    save: async (goal, due) => (sent.push(due), ""),
    oncancel: () => {},
  });
  try {
    // No status yet: the browser day (Los Angeles).
    assert.equal(view.target.querySelector("input[type=date]").value, "2026-11-20");
    // The status answers with the owner zone: the same instant is Nov 21 in Tokyo.
    access.zone = "Asia/Tokyo";
    view.flushSync();
    assert.equal(view.target.querySelector("input[type=date]").value, "2026-11-21");
    // Save with no change keeps the instant.
    submit(view);
    await until(() => sent.length === 1);
    assert.equal(sent[0], old);
    // After the person picks a day, a later zone change does not move the field.
    fill(view, "input[type=date]", "2026-11-25");
    access.zone = "UTC";
    view.flushSync();
    assert.equal(view.target.querySelector("input[type=date]").value, "2026-11-25");
  } finally {
    view.destroy();
    access.zone = keep;
  }
});
