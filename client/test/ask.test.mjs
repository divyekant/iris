// The Ask view (card R5-09): the marks of an answer, the cost words, the
// route, and the conversation in a DOM with a fake engine.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

const fx = await load("/dev/fixtures.ts");
const { parts, cost, thread } = await load("/src/ask.svelte.ts");
const { parse, NAV } = await load("/src/route.ts");
const text = (el) => el.textContent.replace(/\s+/g, " ").trim();
const src = (n, id) => ({ n, quote: null, message: { id, thread: "t-1", date: null, from: { name: "Ana" }, subject: "Hi", labels: [] } });

test("a mark with a source is a part, other marks stay as text", () => {
  const s = [src(1, "m-1"), src(2, "m-2")];
  assert.deepEqual(parts("Lunch at noon [1]. Room 4 [2][7] or [12] [1]", s), [
    { text: "Lunch at noon" },
    { source: s[0] },
    { text: ". Room 4" },
    { source: s[1] },
    { text: "[7] or [12]" },
    { source: s[0] },
  ]);
  assert.deepEqual(parts("No source [3].", s), [{ text: "No source [3]." }]);
  assert.deepEqual(parts("", s), []);
});

test("the cost is in cents, or in dollars from 1 dollar", () => {
  assert.equal(cost(null), "");
  assert.equal(cost(0), "less than 0.1 cents");
  assert.equal(cost(4_180), "0.4 cents");
  assert.equal(cost(10_000), "1 cent");
  assert.equal(cost(994_999), "99.5 cents");
  assert.equal(cost(499), "less than 0.1 cents");
  assert.equal(cost(500), "0.1 cents");
  assert.equal(cost(999_499), "99.9 cents");
  assert.equal(cost(999_500), "$1.00");
  assert.equal(cost(12_340_000), "$12.34");
});

test("#/ask opens the Ask view, which has a link in the header", () => {
  assert.deepEqual(parse("#/ask"), { view: "ask" });
  assert.ok(NAV.some(([link, label]) => link === "#/ask" && label === "Ask"));
});

test("each source of a fixture answer opens its mail, with the same head and the quote", () => {
  for (const q of ["Q4 numbers?", "When does my flight leave?", "What is the weather?"]) {
    for (const source of fx.askAnswer(q).sources) {
      const mail = fx.answer(`messages/${source.message.id}`);
      assert.ok(mail, `${q}: ${source.message.id}`);
      for (const key of ["id", "thread", "date", "from", "subject", "labels"]) {
        assert.deepEqual(mail[key], source.message[key], `${q}: ${source.message.id} ${key}`);
      }
      if (source.quote) assert.ok(mail.text.includes(source.quote), `${q}: ${source.quote}`);
    }
  }
});

// Answer POST /api/ask from `answers`, in order. Keep each question.
function serve(answers) {
  const asked = [];
  globalThis.fetch = async (url, init) => {
    assert.equal(String(url), "/api/ask");
    asked.push(JSON.parse(init.body).question);
    const [status, body] = await answers.shift()();
    return new Response(JSON.stringify(body), { status, headers: { "content-type": "application/json" } });
  };
  return asked;
}

async function send(view, question) {
  const input = view.target.querySelector("input");
  input.value = question;
  input.dispatchEvent(new Event("input", { bubbles: true }));
  view.flushSync();
  view.target.querySelector("form").dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
  view.flushSync();
}

test("questions and answers stack, and each mark opens its source mail", async () => {
  thread.turns.length = 0;
  let release;
  const waits = new Promise((r) => (release = r));
  const asked = serve([
    async () => (await waits, [200, fx.askAnswer("Q4 numbers?")]),
    () => [200, { answer: null, sources: [], read: 0, cost_micro_usd: null }],
    () => [200, { answer: null, sources: [], read: 7, cost_micro_usd: 900 }],
    () => [503, { error: "The budget cannot pay for this question." }],
  ]);
  const view = await mount("/src/views/Ask.svelte");
  const replies = () => [...view.target.querySelectorAll(".reply")];
  try {
    assert.match(text(view.target), /Ask your mail/);
    await send(view, "  Did Priya get the Q4 numbers?  ");
    // One question at a time: the box is empty and the button waits.
    assert.equal(view.target.querySelector("input").value, "");
    assert.match(text(replies()[0]), /Iris reads your mail\./);
    release();
    await until(() => view.target.querySelector("a.cite"));
    view.flushSync();
    const marks = [...view.target.querySelectorAll("a.cite")];
    assert.deepEqual(marks.map((a) => a.getAttribute("href")), ["#/mail/m-55210", "#/mail/m-55211"]);
    assert.equal(marks[0].getAttribute("aria-label"), "Source 1: Priya Raman: Q4 numbers for the board");
    assert.deepEqual([...view.target.querySelectorAll(".chip")].map((a) => text(a)), ["1Priya Raman: Q4 numbers for the board", "2You: Re: Q4 numbers for the board"]);
    assert.match(text(replies()[0]), /Iris read 9 mails\. 0\.4 cents\.$/);

    await send(view, "What is the weather?");
    await until(() => text(replies()[1]).includes("No mail has the words"));
    await send(view, "Where is the key?");
    await until(() => text(replies()[2]).includes("does not answer"));
    view.flushSync();
    assert.match(text(replies()[2]), /Iris read 7 mails\. 0\.1 cents\.$/);

    // An error shows in place, and the question comes back to the box.
    await send(view, "Why so expensive?");
    await until(() => view.target.querySelector("[role=alert]"));
    view.flushSync();
    assert.equal(text(view.target.querySelector("[role=alert]")), "The budget cannot pay for this question.");
    assert.equal(view.target.querySelector("input").value, "Why so expensive?");
    assert.deepEqual(asked, ["Did Priya get the Q4 numbers?", "What is the weather?", "Where is the key?", "Why so expensive?"]);
    // The older answers are muted, the newest is not.
    assert.equal(view.target.querySelectorAll("li.older").length, 3);
  } finally {
    view.destroy();
  }
  // The thread stays when the view goes, for the way back from a source mail.
  const again = await mount("/src/views/Ask.svelte");
  try {
    assert.equal(again.target.querySelectorAll(".question").length, 4);
  } finally {
    again.destroy();
    thread.turns.length = 0;
  }
});

test("a question that fails after the view closed comes back to the box", async () => {
  thread.turns.length = 0;
  thread.draft = "";
  let release;
  const waits = new Promise((r) => (release = r));
  serve([async () => (await waits, [503, { error: "The budget cannot pay for this question." }])]);
  const view = await mount("/src/views/Ask.svelte");
  await send(view, "Where is my passport?");
  assert.equal(view.target.querySelector("input").value, "");
  // The person opens a source mail while the question waits.
  view.destroy();
  release();
  await until(() => !thread.busy);
  const again = await mount("/src/views/Ask.svelte");
  try {
    again.flushSync();
    assert.equal(again.target.querySelector("input").value, "Where is my passport?");
    assert.equal(text(again.target.querySelector("[role=alert]")), "The budget cannot pay for this question.");
  } finally {
    again.destroy();
    thread.turns.length = 0;
    thread.draft = "";
  }
});
