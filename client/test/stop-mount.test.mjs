// Stop (card R4-11): the icon button in the header, the band while stopped, and Resume.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

const text = (el) => el.textContent.replace(/\s+/g, " ");

function serve(answer) {
  const asked = [];
  globalThis.fetch = async (url, init) => {
    const path = String(url).replace(/^\/api\//, "");
    asked.push([init?.method ?? "GET", path]);
    const [status, out] = answer(init?.method ?? "GET", path);
    return new Response(JSON.stringify(out), { status, headers: { "content-type": "application/json" } });
  };
  return asked;
}

const since = "2026-11-17T09:00:00Z";

test("Stop is one icon button; only a complete stop says that no mail leaves; Resume runs again", async () => {
  let stop = { stopped: true, since, sessions: 1, live: 1, busy: false };
  const asked = serve((method, path) =>
    path === "resume" ? [200, { stopped: false, since: null, resumed: true }] : method === "POST" ? [200, stop] : [200, { stopped: false, since: null, live: 0, busy: false }],
  );
  const button = await mount("/src/views/Stop.svelte", { place: "button" });
  const band = await mount("/src/views/Stop.svelte", { place: "band" });
  try {
    const stopButton = button.target.querySelector("button.stop-all");
    assert.equal(stopButton.getAttribute("aria-label"), "Stop all agent work");
    assert.equal(text(stopButton).trim(), "", "the button has no text, only the icon");
    stopButton.click();
    await until(() => band.target.querySelector("[role=status]"));
    assert.deepEqual(asked.at(-1), ["POST", "stop"]);
    assert.equal(button.target.querySelector("button.stop-all"), null, "no Stop button while stopped");
    // One session still stops: no claim that no mail leaves.
    assert.match(text(band.target), /The stop is not complete yet/);
    assert.doesNotMatch(text(band.target), /no mail leaves/);
    stop = { ...stop, live: 0 };
    [...band.target.querySelectorAll("button")].find((b) => text(b).includes("Stop again")).click();
    await until(() => /no mail leaves/.test(text(band.target)));
    [...band.target.querySelectorAll("button")].find((b) => text(b).includes("Resume")).click();
    await until(() => button.target.querySelector("button.stop-all"));
    assert.deepEqual(asked.at(-1), ["POST", "resume"]);
    assert.match(text(band.target), /Each held send waits a full minute/);
  } finally {
    button.destroy();
    band.destroy();
  }
});

test("refresh reads a stop that is busy, and the band says that it is not complete", async () => {
  serve(() => [200, { stopped: true, since, live: 0, busy: true }]);
  const band = await mount("/src/views/Stop.svelte", { place: "band" });
  try {
    const mod = await load("/src/views/Stop.svelte");
    await mod.refresh();
    await until(() => band.target.querySelector("[role=status]"));
    assert.match(text(band.target), /The stop is not complete yet/);
    assert.match(text(band.target), /A Gmail change was in progress/);
  } finally {
    band.destroy();
  }
});

test("an older answer never replaces a newer state", async () => {
  const { refresh, act, stopState } = await load("/src/views/Stop.svelte");
  Object.assign(stopState, { stopped: true, since, live: 0, busy: false, note: "", error: "" });
  // The GET waits; the resume answers first.
  let release;
  const late = new Promise((r) => (release = r));
  globalThis.fetch = async (url, init) => {
    const path = String(url).replace(/^\/api\//, "");
    const out = path === "resume" ? { stopped: false, since: null, resumed: true } : (await late, { stopped: true, since, live: 0, busy: false });
    return new Response(JSON.stringify(out), { status: 200, headers: { "content-type": "application/json" } });
  };
  const band = await mount("/src/views/Stop.svelte", { place: "band" });
  try {
    const reading = refresh();
    await act("resume");
    assert.equal(stopState.stopped, false);
    release();
    await reading;
    band.flushSync();
    assert.equal(stopState.stopped, false, "the late GET did not stop Iris again on the screen");
    assert.doesNotMatch(text(band.target), /no mail leaves/);
  } finally {
    band.destroy();
  }
});

test("a stop with only a Gmail call in progress offers Stop again, and the tooltip makes no claim on mail", async () => {
  const { stopState } = await load("/src/views/Stop.svelte");
  Object.assign(stopState, { stopped: false, since: null, live: 0, busy: false, note: "", error: "" });
  serve((method) => [200, method === "POST" ? { stopped: true, since, sessions: 0, live: 0, busy: true } : { stopped: false, since: null, live: 0, busy: false }]);
  const button = await mount("/src/views/Stop.svelte", { place: "button" });
  const band = await mount("/src/views/Stop.svelte", { place: "band" });
  try {
    const stopButton = button.target.querySelector("button.stop-all");
    assert.doesNotMatch(stopButton.getAttribute("title"), /mail/);
    stopButton.click();
    await until(() => band.target.querySelector("[role=status]"));
    assert.match(text(band.target), /The stop is not complete yet/);
    assert.ok([...band.target.querySelectorAll("button")].some((b) => text(b).includes("Stop again")));
  } finally {
    button.destroy();
    band.destroy();
  }
});

test("a Resume with no answer makes no claim, and a read of the engine gives the state", async () => {
  const { stopState } = await load("/src/views/Stop.svelte");
  Object.assign(stopState, { stopped: true, since, live: 0, busy: false, known: true, note: "", error: "" });
  let getWorks = false;
  globalThis.fetch = async (url, init) => {
    const path = String(url).replace(/^\/api\//, "");
    // The resume commits in the engine, but its answer is lost.
    if (path === "resume" || !getWorks) throw new TypeError("Failed to fetch");
    return new Response(JSON.stringify({ stopped: false, since: null, live: 0, busy: false }), { status: 200, headers: { "content-type": "application/json" } });
  };
  const band = await mount("/src/views/Stop.svelte", { place: "band" });
  try {
    [...band.target.querySelectorAll("button")].find((b) => text(b).includes("Resume")).click();
    await until(() => stopState.known === false && stopState.busyAction === "");
    band.flushSync();
    assert.doesNotMatch(text(band.target), /no mail leaves/);
    assert.match(text(band.target), /does not know if agent work is stopped/);
    assert.doesNotMatch(text(band.target), /Iris is stopped/);
    // The next read of the engine gives the state, and clears the old error.
    getWorks = true;
    const { refresh } = await load("/src/views/Stop.svelte");
    await refresh();
    band.flushSync();
    assert.equal(stopState.stopped, false);
    assert.equal(stopState.known, true);
    assert.equal(stopState.error, "");
    assert.equal(band.target.querySelector("[role=status]")?.getAttribute("aria-label") ?? null, null);
  } finally {
    band.destroy();
  }
});

test("a Resume when Iris already runs does not say that a new hold started", async () => {
  const { act, stopState } = await load("/src/views/Stop.svelte");
  Object.assign(stopState, { stopped: true, since, live: 0, busy: false, known: true, note: "", error: "" });
  serve(() => [200, { stopped: false, since: null, resumed: false }]);
  await act("resume");
  assert.equal(stopState.stopped, false);
  assert.equal(stopState.note, "");
});

test("a failed read after a complete stop takes back the claim, and the Stop button comes back", async () => {
  const { refresh, stopState } = await load("/src/views/Stop.svelte");
  Object.assign(stopState, { stopped: true, since, live: 0, busy: false, known: true, note: "", error: "" });
  globalThis.fetch = async () => {
    throw new TypeError("Failed to fetch");
  };
  const button = await mount("/src/views/Stop.svelte", { place: "button" });
  const band = await mount("/src/views/Stop.svelte", { place: "band" });
  try {
    assert.match(text(band.target), /no mail leaves/);
    // For example, `iris resume` ran at the terminal, and the engine does not answer now.
    await refresh();
    band.flushSync();
    assert.doesNotMatch(text(band.target), /no mail leaves|Iris is stopped/);
    assert.match(text(band.target), /does not know if agent work is stopped/);
    assert.ok(button.target.querySelector("button.stop-all"));
  } finally {
    button.destroy();
    band.destroy();
  }
});

test("a Stop with no answer makes no claim", async () => {
  const { act, stopState } = await load("/src/views/Stop.svelte");
  Object.assign(stopState, { stopped: false, since: null, live: 0, busy: false, known: true, note: "", error: "" });
  globalThis.fetch = async () => {
    throw new TypeError("Failed to fetch");
  };
  const band = await mount("/src/views/Stop.svelte", { place: "band" });
  try {
    await act("stop");
    band.flushSync();
    assert.equal(stopState.known, false);
    assert.doesNotMatch(text(band.target), /no mail leaves|Iris is stopped/);
    assert.match(text(band.target), /does not know if agent work is stopped/);
  } finally {
    band.destroy();
  }
});
