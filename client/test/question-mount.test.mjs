// The question of an agent in the brief (card R5-05): the answer goes to the
// engine once, and each result of the engine shows in words.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

const fx = await load("/dev/fixtures.ts");
const text = (el) => el.textContent.replace(/\s+/g, " ");
const job = () => JSON.parse(JSON.stringify(fx.jobs["j-14"]));

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
    assert.deepEqual(asked, [["POST", "jobs/j-14/answer", { answer: "Ask Brightline first." }]]);
  } finally {
    view.destroy();
  }
});

test("a job that asks no question now loads the questions again", async () => {
  serve(() => [409, { error: "The job asks no question now." }]);
  let changed = 0;
  const view = await mount("/src/views/Question.svelte", { job: job(), onanswered: () => {}, onchanged: () => changed++ });
  try {
    await type(view, "Yes.");
    button(view).click();
    await until(() => view.target.querySelector("[role=alert]"));
    assert.match(text(view.target), /This job asks no question now\./);
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
    // The answer of the last question leaves the line and no card.
    left = [];
    await type(view, "The blue one.");
    button(view).click();
    await until(() => /“Renew the passport\.”/.test(text(view.target)));
    assert.equal(view.target.querySelector("textarea"), null);
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
  const [code, doc] = fx.answerJob("j-14", given);
  assert.equal(code, 200);
  assert.equal(doc.job.events.at(-1).answer, given);
  assert.equal(fx.answerJob("j-14", "again")[0], 409);
  assert.equal(fx.answerJob("j-15", "  ")[0], 400);
});
