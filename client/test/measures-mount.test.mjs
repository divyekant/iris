// The measures page (R5-12) in a DOM: a direct visit writes the use of today before it reads the measures.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

const fx = await load("/dev/fixtures.ts");

test("the measures page reads the measures after the use of today and each open in flight are written", async () => {
  const asked = [];
  let written = null;
  const saved = globalThis.fetch;
  const names = ["location", "history", "addEventListener", "removeEventListener", "scrollTo"];
  const before = names.map((n) => globalThis[n]);
  for (const n of names) globalThis[n] = typeof window[n] === "function" ? window[n].bind(window) : window[n];
  window.location.hash = "#/measures";
  globalThis.fetch = (url, init) => {
    const path = String(url).replace(/^\/api\//, "");
    asked.push(`${init?.method ?? "GET"} ${path}`);
    if (path === "measures/open") return new Promise((r) => (opened = () => r(new Response("{}", { status: 200 }))));
    if (path === "measures/use") return new Promise((r) => (written = () => r(new Response("{}", { status: 200 }))));
    const body = fx.answer(`/${path}`);
    return Promise.resolve(new Response(JSON.stringify(body ?? { error: "Not found." }), { status: body === undefined ? 404 : 200 }));
  };
  // An open of a mail just before: its count is still in flight.
  let opened = null;
  const { count } = await load("/src/measures.ts");
  const open = count({ open: "m-1" });
  await until(() => opened !== null);
  const app = await mount("/src/App.svelte");
  try {
    await until(() => written !== null);
    // The use waits: the page has not asked for the measures.
    await new Promise((r) => setTimeout(r, 20));
    assert.equal(asked.filter((a) => a === "GET measures").length, 0);
    assert.equal(asked.filter((a) => a === "POST measures/use").length, 1);
    written();
    // The use has its answer, and the open does not: still no read.
    await new Promise((r) => setTimeout(r, 20));
    assert.equal(asked.filter((a) => a === "GET measures").length, 0);
    opened();
    await open;
    await until(() => app.target.querySelector(".rows li") !== null);
    assert.ok(asked.indexOf("GET measures") > asked.indexOf("POST measures/use"));
  } finally {
    app.destroy();
    globalThis.fetch = saved;
    names.forEach((n, i) => (globalThis[n] = before[i]));
  }
});
