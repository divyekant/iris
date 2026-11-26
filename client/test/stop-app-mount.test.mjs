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

test("#/write/d-N of a draft that cannot change shows why and a link, with no form (issue 941)", async () => {
  const DRAFT = { draft: "d-7", account: "a-1", thread: "t-2", to: [], cc: [], bcc: [], subject: "Re: Q4", body: "Old.\n", attachments: [], change: null };
  let newer = true;
  globalThis.fetch = (url) => {
    const path = String(url).replace(/^\/api\//, "");
    const out = path === "drafts/d-7" ? { ...DRAFT, newer_mail: newer } : path === "stop" ? { stopped: false, since: null, live: 0, busy: false } : null;
    if (!out) return new Promise(() => {});
    return Promise.resolve(new Response(JSON.stringify(out), { status: 200, headers: { "content-type": "application/json" } }));
  };
  for (const name of ["location", "addEventListener", "removeEventListener", "scrollTo"]) {
    globalThis[name] ??= typeof window[name] === "function" ? window[name].bind(window) : window[name];
  }
  for (const [n, words, link] of [[true, /A new mail came into this thread\. Write a new reply\./, "#/write/t-2"], [false, /This draft cannot change now\./, "#/write"]]) {
    newer = n;
    location.hash = "#/write/d-7";
    const app = await mount("/src/App.svelte");
    try {
      await until(() => app.target.querySelector("main p.problem a"));
      const p = app.target.querySelector("main p.problem");
      assert.match(p.textContent, words);
      assert.equal(p.querySelector("a").getAttribute("href"), link);
      assert.equal(app.target.querySelector("main form"), null, "no form");
    } finally {
      app.destroy();
    }
  }
});
