// "Try now" (decision D35, issue 136): the brief says when Iris tries the
// model again after errors, and the button ends the wait.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount } from "./mount.mjs";

const fx = await load("/dev/fixtures.ts");
const text = (el) => el.textContent.replace(/\s+/g, " ").trim();
const NOW = new Date("2026-11-16T09:05:00Z");
const tick = () => new Promise((r) => setTimeout(r, 0));

test("the brief shows the model wait only with a retry time, and Try now posts and ends it", async () => {
  const saved = globalThis.fetch;
  const asked = [];
  let answer = [200, { ...fx.status }];
  globalThis.fetch = async (url, init) => {
    asked.push([String(url), init?.body ? JSON.parse(init.body) : null]);
    return new Response(JSON.stringify(answer[1]), { status: answer[0], headers: { "content-type": "application/json" } });
  };
  const none = await mount("/src/views/Brief.svelte", { brief: fx.brief, status: fx.status, now: NOW, poll: 0 });
  try {
    assert.equal(none.target.querySelector(".model-wait"), null);
  } finally {
    none.destroy();
  }
  const status = { ...fx.status, model_retry_at: "2026-11-16T09:20:00Z" };
  const view = await mount("/src/views/Brief.svelte", { brief: fx.brief, status, now: NOW, poll: 0 });
  try {
    const note = view.target.querySelector(".model-wait");
    assert.match(text(note), /The model failed, so Iris waits before it reads more mail\. It tries again at .+\. Try now/);
    // A refused call keeps the note and shows the error.
    answer = [500, { error: "The engine is busy." }];
    note.querySelector("button").click();
    for (let i = 0; i < 5; i++) await tick();
    assert.match(text(view.target.querySelector(".model-wait")), /The engine is busy\./);
    answer = [200, { ...fx.status }];
    view.target.querySelector(".model-wait button").click();
    for (let i = 0; i < 5; i++) await tick();
    assert.deepEqual(asked.filter(([u]) => u === "/api/model/try"), [["/api/model/try", {}], ["/api/model/try", {}]]);
    assert.equal(view.target.querySelector(".model-wait"), null);
  } finally {
    view.destroy();
    globalThis.fetch = saved;
  }
});
