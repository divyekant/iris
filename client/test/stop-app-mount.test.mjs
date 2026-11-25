// Stop (card R4-11) in the app: the button in the header, and the band above each view.
import { mock, test } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

test("the app has the Stop button in the header, and the band above each view after a route change", async () => {
  let stopped = false;
  globalThis.fetch = (url) => {
    const path = String(url).replace(/^\/api\//, "");
    // Only the Stop state answers; the other calls of the views stay pending.
    if (path !== "stop") return new Promise(() => {});
    const out = { stopped, since: stopped ? "2026-11-17T09:00:00Z" : null, live: 0, busy: false };
    return Promise.resolve(new Response(JSON.stringify(out), { status: 200, headers: { "content-type": "application/json" } }));
  };
  // The globals that App.svelte uses, from the DOM of mount.mjs.
  for (const name of ["location", "addEventListener", "removeEventListener", "scrollTo"]) {
    globalThis[name] ??= typeof window[name] === "function" ? window[name].bind(window) : window[name];
  }
  location.hash = "#/brief";
  const app = await mount("/src/App.svelte");
  try {
    await until(() => app.target.querySelector("header button.stop-all"));
    stopped = true;
    location.hash = "#/history";
    window.dispatchEvent(new Event("hashchange"));
    await until(() => app.target.querySelector('main [role=status][aria-label="Iris is stopped"]'));
    assert.equal(app.target.querySelector("header button.stop-all"), null);
  } finally {
    app.destroy();
    const { stopState } = await load("/src/views/Stop.svelte");
    Object.assign(stopState, { stopped: false, since: null, known: true });
  }
});

test("an open app shows a stop at the terminal within 30 seconds, with no navigation or focus (issue 589)", async () => {
  let stopped = false;
  let reads = 0;
  globalThis.fetch = (url) => {
    const path = String(url).replace(/^\/api\//, "");
    if (path !== "stop") return new Promise(() => {});
    reads += 1;
    const out = { stopped, since: stopped ? "2026-11-17T09:00:00Z" : null, live: 0, busy: false };
    return Promise.resolve(new Response(JSON.stringify(out), { status: 200, headers: { "content-type": "application/json" } }));
  };
  for (const name of ["location", "addEventListener", "removeEventListener", "scrollTo"]) {
    globalThis[name] ??= typeof window[name] === "function" ? window[name].bind(window) : window[name];
  }
  // Only the interval timers are fake: `until` still waits on real time.
  mock.timers.enable({ apis: ["setInterval"] });
  location.hash = "#/brief";
  const app = await mount("/src/App.svelte");
  const band = () => app.target.querySelector('main [role=status][aria-label="Iris is stopped"]');
  try {
    await until(() => app.target.querySelector("header button.stop-all"));
    // Let the reads of the start and of the first view end.
    await new Promise((resolve) => setTimeout(resolve, 100));
    // The terminal stops Iris. Nothing in the tab changes the view or the focus.
    stopped = true;
    const before = reads;
    mock.timers.tick(29_999);
    await new Promise((resolve) => setTimeout(resolve, 50));
    assert.equal(reads, before, "no read before 30 seconds");
    assert.equal(band(), null);
    mock.timers.tick(1);
    await until(band);
    assert.equal(app.target.querySelector("header button.stop-all"), null);
  } finally {
    app.destroy();
    mock.timers.reset();
    const { stopState } = await load("/src/views/Stop.svelte");
    Object.assign(stopState, { stopped: false, since: null, known: true });
  }
});
