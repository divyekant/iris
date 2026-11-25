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
  // The wait ends one hour from the real clock: the note stays during the test.
  const status = { ...fx.status, model_retry_at: new Date(Date.now() + 3_600_000).toISOString() };
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

test("the model wait note goes when its time comes, and the brief reads the status again", async () => {
  const saved = globalThis.fetch;
  const asked = [];
  globalThis.fetch = async (url) => {
    asked.push(String(url));
    const body = String(url) === "/api/status" ? { ...fx.status } : [];
    return new Response(JSON.stringify(body), { status: 200, headers: { "content-type": "application/json" } });
  };
  const at = new Date(Date.now() + 50).toISOString();
  const view = await mount("/src/views/Brief.svelte", { brief: fx.brief, status: { ...fx.status, model_retry_at: at }, now: NOW, poll: 0 });
  try {
    assert.notEqual(view.target.querySelector(".model-wait"), null);
    await new Promise((r) => setTimeout(r, 150));
    for (let i = 0; i < 5; i++) await tick();
    assert.equal(view.target.querySelector(".model-wait"), null);
    assert.ok(asked.includes("/api/status"), String(asked));
  } finally {
    view.destroy();
    globalThis.fetch = saved;
  }
});

test("a slow poll that started before Try now does not bring the wait note back", async () => {
  const saved = globalThis.fetch;
  const waiting = { ...fx.status, model_retry_at: new Date(Date.now() + 3_600_000).toISOString() };
  let release;
  let polls = 0;
  globalThis.fetch = async (url, init) => {
    const u = String(url);
    const json = (body) => new Response(JSON.stringify(body), { status: 200, headers: { "content-type": "application/json" } });
    if (u === "/api/model/try") return json({ ...fx.status });
    if (u === "/api/status") {
      polls += 1;
      // The first poll reads the old status and answers only after the try.
      if (polls === 1) {
        await new Promise((r) => (release = r));
        return json(waiting);
      }
      return json({ ...fx.status });
    }
    return json(u.startsWith("/api/brief") ? fx.brief : []);
  };
  const view = await mount("/src/views/Brief.svelte", { brief: fx.brief, status: waiting, now: NOW, poll: 10 });
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  try {
    // Not syncing: the brief polls each 12 * 10 ms.
    for (let i = 0; i < 40 && polls === 0; i++) await wait(10);
    assert.equal(polls, 1, "the first poll waits");
    view.target.querySelector(".model-wait button").click();
    await wait(10);
    assert.equal(view.target.querySelector(".model-wait"), null);
    release();
    await wait(5);
    assert.equal(view.target.querySelector(".model-wait"), null);
  } finally {
    view.destroy();
    globalThis.fetch = saved;
  }
});

test("the read at the end of a wait does not hide a newer wait that a later poll showed", async () => {
  const saved = globalThis.fetch;
  const later = new Date(Date.now() + 3_600_000).toISOString();
  let release;
  let reads = 0;
  globalThis.fetch = async (url) => {
    const u = String(url);
    const json = (body) => new Response(JSON.stringify(body), { status: 200, headers: { "content-type": "application/json" } });
    if (u === "/api/status") {
      reads += 1;
      // The first read is the read at the end of the wait: it reads no wait, and answers last.
      if (reads === 1) {
        await new Promise((r) => (release = r));
        return json({ ...fx.status });
      }
      // The poll after it: the model failed again, and a new wait runs.
      return json({ ...fx.status, model_retry_at: later });
    }
    return json(u.startsWith("/api/brief") ? fx.brief : []);
  };
  const at = new Date(Date.now() + 20).toISOString();
  const view = await mount("/src/views/Brief.svelte", { brief: fx.brief, status: { ...fx.status, model_retry_at: at }, now: NOW, poll: 10 });
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  try {
    for (let i = 0; i < 60 && reads < 2; i++) await wait(10);
    assert.equal(reads, 2, "the poll read after the read at the end of the wait");
    await wait(5);
    assert.notEqual(view.target.querySelector(".model-wait"), null);
    release();
    await wait(5);
    assert.notEqual(view.target.querySelector(".model-wait"), null, "the older read does not hide the newer wait");
  } finally {
    view.destroy();
    globalThis.fetch = saved;
  }
});

test("a Try now answer that comes after a newer wait does not hide it", async () => {
  const saved = globalThis.fetch;
  const waiting = { ...fx.status, model_retry_at: new Date(Date.now() + 3_600_000).toISOString() };
  const again = { ...fx.status, model_retry_at: new Date(Date.now() + 7_200_000).toISOString() };
  let release;
  let reads = 0;
  globalThis.fetch = async (url) => {
    const u = String(url);
    const json = (body) => new Response(JSON.stringify(body), { status: 200, headers: { "content-type": "application/json" } });
    // The try ends the wait, then answers late. Meanwhile the model fails again: each read gives a new wait.
    if (u === "/api/model/try") {
      await new Promise((r) => (release = r));
      return json({ ...fx.status });
    }
    if (u === "/api/status") {
      reads += 1;
      return json(again);
    }
    return json(u.startsWith("/api/brief") ? fx.brief : []);
  };
  const view = await mount("/src/views/Brief.svelte", { brief: fx.brief, status: waiting, now: NOW, poll: 10 });
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  try {
    view.target.querySelector(".model-wait button").click();
    // Not syncing: the brief polls each 12 * 10 ms.
    for (let i = 0; i < 60 && reads < 1; i++) await wait(10);
    assert.equal(reads, 1, "a poll read the new wait during the try");
    release();
    for (let i = 0; i < 10; i++) await tick();
    await wait(5);
    assert.notEqual(view.target.querySelector(".model-wait"), null, "the newer wait stays");
  } finally {
    view.destroy();
    globalThis.fetch = saved;
  }
});
