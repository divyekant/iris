// Stop (card R4-11) in the app: the button in the header, and the band above each view.
import { test } from "node:test";
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
