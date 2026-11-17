// The question of an agent in the brief (card R5-05): the answer goes to the
// engine once, and each result of the engine shows in words.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

const fx = await load("/dev/fixtures.ts");
const text = (el) => el.textContent.replace(/\s+/g, " ");
// A copy from the start: a test of the fixture answer changes the fixture job.
const asking = JSON.stringify(fx.jobs["j-14"]);
const job = () => JSON.parse(asking);

// Answer each request with `answer(path, body)`: [status, body].
function serve(answer) {
  const asked = [];
  globalThis.fetch = async (url, init) => {
    const path = String(url).replace(/^\/api\//, "");
    const body = init?.body ? JSON.parse(init.body) : undefined;
    asked.push([init?.method ?? "GET", path, body]);
    const [status, out] = await answer(path, body);
    return new Response(JSON.stringify(out), { status, headers: { "content-type": "application/json" } });
  };
  return asked;
}

async function type(view, value) {
  const field = view.target.querySelector("textarea");
  field.value = value;
  field.dispatchEvent(new Event("input", { bubbles: true }));
  view.flushSync();
}
const button = (view) => view.target.querySelector("button[type=submit]");

test("the card shows the goal, the question as text, and sends the answer once", async () => {
  const asked = serve(() => [200, { job: { ...job(), state: "working" }, changesets: [], threads: {} }]);
  const q = { ...job(), question: "<b>Pay</b> the full amount?" };
  let answered = null;
  const view = await mount("/src/views/Question.svelte", { job: q, onanswered: (j) => (answered = j.id), onchanged: () => {} });
  try {
    assert.match(text(view.target), /An agent asks you, for the job Pay the Q4 invoice from Brightline\./);
    assert.equal(view.target.querySelector("a").getAttribute("href"), "#/jobs/j-14");
    // The question of the agent is text, never markup.
    assert.equal(view.target.querySelector("b"), null);
    assert.match(text(view.target), /<b>Pay<\/b> the full amount\?/);
    assert.equal(button(view).disabled, true);
    await type(view, "   ");
    assert.equal(button(view).disabled, true);
    await type(view, "Ask Brightline first.");
    assert.equal(button(view).disabled, false);
    button(view).click();
    await until(() => answered);
    assert.equal(answered, "j-14");
    assert.deepEqual(asked, [["POST", "jobs/j-14/answer", { answer: "Ask Brightline first.", question: q.question, updated: q.updated }]]);
  } finally {
    view.destroy();
  }
});

test("the limit counts characters, so a long answer of emoji goes in full", async () => {
  const asked = serve(() => [200, { job: { ...job(), state: "working" }, changesets: [], threads: {} }]);
  let answered = false;
  const view = await mount("/src/views/Question.svelte", { job: job(), onanswered: () => (answered = true), onchanged: () => {} });
  try {
    // 6,000 characters outside the Basic Multilingual Plane are 12,000 UTF-16 units.
    const emoji = "\u{1F600}".repeat(6000);
    assert.equal(view.target.querySelector("textarea").hasAttribute("maxlength"), false);
    await type(view, emoji);
    button(view).click();
    await until(() => answered);
    assert.deepEqual(asked, [["POST", "jobs/j-14/answer", { answer: emoji, question: job().question, updated: job().updated }]]);
    asked.length = 0;
    await type(view, "a".repeat(10_001));
    button(view).click();
    await until(() => /longer than 10,000 characters/.test(text(view.target)));
    assert.deepEqual(asked, []);
    assert.equal(view.target.querySelector("textarea").value.length, 10_001);
  } finally {
    view.destroy();
  }
});

test("a job that asks no question now loads the questions again", async () => {
  serve(() => [409, { error: "The job does not ask this question now." }]);
  let changed = 0;
  const view = await mount("/src/views/Question.svelte", { job: job(), onanswered: () => {}, onchanged: () => changed++ });
  try {
    await type(view, "Yes.");
    button(view).click();
    await until(() => view.target.querySelector("[role=alert]"));
    assert.match(text(view.target), /This job does not ask this question now\./);
    assert.equal(changed, 1);
  } finally {
    view.destroy();
  }
});

test("a refusal keeps the answer, and no answer says that Iris does not know", async () => {
  serve(() => [500, { error: "database: locked" }]);
  const view = await mount("/src/views/Question.svelte", { job: job(), onanswered: () => {}, onchanged: () => {} });
  try {
    await type(view, "Yes.");
    button(view).click();
    await until(() => view.target.querySelector("[role=alert]"));
    assert.match(text(view.target), /database: locked/);
    assert.equal(view.target.querySelector("textarea").value, "Yes.");
    globalThis.fetch = async () => {
      throw new TypeError("Failed to fetch");
    };
    button(view).click();
    await until(() => /does not know|cannot reach/.test(text(view.target)));
  } finally {
    view.destroy();
  }
});

test("the brief shows the newest question, and links the others", async () => {
  serve(() => [200, { jobs: [], next: null }]);
  const second = { ...job(), id: "j-13", goal: "Renew the passport.", question: "Which photo?" };
  const view = await mount("/src/views/Brief.svelte", { watches: [], proposed: null, questions: [job(), second], brief: fx.brief, now: new Date("2026-11-16T09:05:00Z"), poll: 0 });
  try {
    const t = text(view.target);
    assert.match(t, /Questions for you 2/);
    assert.match(t, /Pay the full amount, or ask Brightline/);
    assert.doesNotMatch(t, /Which photo\?/);
    assert.ok([...view.target.querySelectorAll("a")].some((a) => a.getAttribute("href") === "#/jobs" && /All 2 questions/.test(a.textContent)));
  } finally {
    view.destroy();
  }
});

test("the questions come newest question first, not newest job first", async () => {
  const { loadQuestions } = await load("/src/jobs.ts");
  const old = { ...job(), id: "j-3", updated: "2026-11-17T09:00:00.000Z", question: "Asked today?" };
  const young = { ...job(), id: "j-30", updated: "2026-11-16T09:00:00.000Z", question: "Asked yesterday?" };
  // The API pages by job ID, newest job first.
  serve(() => [200, { jobs: [young, old], next: null }]);
  assert.deepEqual((await loadQuestions()).map((j) => j.id), ["j-3", "j-30"]);
});

test("after an answer, the brief says so and shows the next question at once", async () => {
  const second = { ...job(), id: "j-13", goal: "Renew the passport.", question: "Which photo?", updated: "2026-11-15T09:00:00.000Z" };
  let left = [second];
  const asked = serve((path) => (path.endsWith("/answer") ? [200, {}] : [200, { jobs: left, next: null }]));
  const view = await mount("/src/views/Brief.svelte", { watches: [], proposed: null, questions: [job(), second], brief: fx.brief, now: new Date("2026-11-16T09:05:00Z"), poll: 0 });
  try {
    await type(view, "Ask Brightline first.");
    button(view).click();
    await until(() => /Which photo\?/.test(text(view.target)));
    assert.match(text(view.target), /Sent\. The agent of “Pay the Q4 invoice from Brightline\.” continues with your answer\./);
    assert.match(text(view.target), /A question for you 1/);
    assert.equal(view.target.querySelector("textarea").value, "");
    assert.ok(asked.some(([m, p]) => m === "GET" && p.startsWith("jobs?state=needs_you")));
    // The answer of the last question leaves the line, and no section.
    left = [];
    await type(view, "The blue one.");
    button(view).click();
    await until(() => /“Renew the passport\.”/.test(text(view.target)));
    assert.equal(view.target.querySelector("textarea"), null);
    assert.equal(view.target.querySelector("#questions"), null);
    assert.doesNotMatch(text(view.target), /Questions? for you/);
  } finally {
    view.destroy();
  }
});

test("a new question of the same job is a new card with an empty field", async () => {
  const next = { ...job(), question: "And the second invoice?", updated: "2026-11-17T11:00:00.000Z" };
  let list = [job()];
  serve((path) => (path === "brief" ? [200, fx.brief] : path === "status" ? [200, fx.answer("status")] : path.startsWith("jobs") ? [200, { jobs: list, next: null }] : [200, path.startsWith("watches") ? [] : { proposed: [], confirmed: [] }]));
  const view = await mount("/src/views/Brief.svelte", { watches: [], proposed: null, questions: [job()], brief: fx.brief, now: new Date("2026-11-16T09:05:00Z"), poll: 5 });
  try {
    await type(view, "Half typed");
    list = [next];
    await until(() => /And the second invoice\?/.test(text(view.target)));
    view.flushSync();
    assert.equal(view.target.querySelector("textarea").value, "");
  } finally {
    view.destroy();
  }
});

test("the newest question can be on a later page", async () => {
  const { loadQuestions } = await load("/src/jobs.ts");
  const at = (h) => `2026-11-17T${String(h).padStart(2, "0")}:00:00.000Z`;
  const first = Array.from({ length: 200 }, (_, k) => ({ ...job(), id: `j-${1000 - k}`, updated: at(1) }));
  const late = { ...job(), id: "j-2", updated: at(9), question: "Asked last?" };
  const asked = serve((path) => [200, path.includes("after=") ? { jobs: [late], next: null } : { jobs: first, next: "j-801" }]);
  const list = await loadQuestions();
  assert.equal(list.length, 201);
  assert.equal(list[0].id, "j-2");
  assert.deepEqual(asked.map(([, p]) => p), ["jobs?state=needs_you&limit=200", "jobs?state=needs_you&limit=200&after=j-801"]);
});

test("the preview keeps an answer as the person wrote it", () => {
  const given = "  card: blue\n  limit: 2400\n";
  const seen = { question: job().question, updated: job().updated };
  // An answer to an older question of the job: the same text at another time.
  assert.equal(fx.answerJob("j-14", { answer: given, ...seen, updated: "2000-01-01T00:00:00.000Z" })[0], 409);
  const [code, doc] = fx.answerJob("j-14", { answer: given, ...seen });
  assert.equal(code, 200);
  assert.equal(doc.job.events.at(-1).answer, given);
  assert.equal(fx.answerJob("j-14", { answer: "again", ...seen })[0], 409);
  assert.equal(fx.answerJob("j-15", { answer: "  " })[0], 400);
});

test("a draft answer stays when a newer question of another job comes", async () => {
  const at = (h) => `2026-11-17T${String(h).padStart(2, "0")}:00:00.000Z`;
  const mine = { ...job(), updated: at(8) };
  const newer = { ...job(), id: "j-20", goal: "Book the flight.", question: "Window or aisle?", updated: at(9) };
  let list = [mine];
  serve((path) => (path === "brief" ? [200, fx.brief] : path === "status" ? [200, fx.answer("status")] : path.startsWith("jobs") ? [200, { jobs: list, next: null }] : [200, path.startsWith("watches") ? [] : { proposed: [], confirmed: [] }]));
  const view = await mount("/src/views/Brief.svelte", { watches: [], proposed: null, questions: [mine], brief: fx.brief, now: new Date("2026-11-16T09:05:00Z"), poll: 5 });
  try {
    await type(view, "Half typed");
    list = [newer, mine];
    await until(() => /Questions for you 2/.test(text(view.target)));
    view.flushSync();
    // The card with the unsent answer stays, with its text.
    assert.match(text(view.target), /Pay the full amount/);
    assert.equal(view.target.querySelector("textarea").value, "Half typed");
    // With no draft, the newest question shows; the draft comes back with its question.
    await type(view, "");
    await until(() => /Window or aisle\?/.test(text(view.target)));
    list = [{ ...mine, updated: at(10) }, newer];
    await until(() => /Pay the full amount/.test(text(view.target)));
  } finally {
    view.destroy();
  }
});

test("the brief loads the questions when it opens with none given", async () => {
  // App mounts the brief with no questions: the brief asks the engine.
  const asked = serve((path) => (path.startsWith("jobs") ? [200, { jobs: [job()], next: null }] : [200, path.startsWith("watches") ? [] : { proposed: [], confirmed: [] }]));
  const view = await mount("/src/views/Brief.svelte", { watches: [], proposed: null, brief: fx.brief, now: new Date("2026-11-16T09:05:00Z"), poll: 0 });
  try {
    await until(() => /Pay the full amount, or ask Brightline/.test(text(view.target)));
    assert.ok(asked.some(([m, p]) => m === "GET" && p === "jobs?state=needs_you&limit=200"));
  } finally {
    view.destroy();
  }
});
