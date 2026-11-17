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
  const view = await mount("/src/views/Question.svelte", { job: q, onchanged: () => {} });
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
    await until(() => view.target.querySelector("[role=status]"));
    assert.match(text(view.target), /Sent\. The agent continues with your answer\./);
    assert.equal(view.target.querySelector("textarea"), null);
    assert.deepEqual(asked, [["POST", "jobs/j-14/answer", { answer: "Ask Brightline first." }]]);
  } finally {
    view.destroy();
  }
});

test("a job that asks no question now loads the questions again", async () => {
  serve(() => [409, { error: "The job asks no question now." }]);
  let changed = 0;
  const view = await mount("/src/views/Question.svelte", { job: job(), onchanged: () => changed++ });
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
  const view = await mount("/src/views/Question.svelte", { job: job(), onchanged: () => {} });
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
