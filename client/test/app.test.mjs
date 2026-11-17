// The app shell shows the Iris name and the navigation to each view.
import { test } from "node:test";
import assert from "node:assert/strict";
import { render } from "./render.mjs";

test("the app shows the Iris name and the views", async () => {
  const html = await render("/src/App.svelte");
  assert.match(html, /<a class="name[^"]*" href="#\/brief">[\s\S]*?Iris\s*<\/a>/);
  for (const view of ["Brief", "Commitments", "Conversations", "Records", "Feed", "Signals", "Measures"]) {
    assert.match(html, new RegExp(`>${view}</a>`));
  }
});

test("the app shows the access problem above each view", async () => {
  const { load } = await import("./render.mjs");
  const { access } = await load("/src/access.svelte.ts");
  // The views start API calls while they render. Keep them pending, so they do not change the state.
  const saved = globalThis.fetch;
  globalThis.fetch = () => new Promise(() => {});
  access.problem = "Open the sign-in address that iris serve printed.";
  try {
    const html = await render("/src/App.svelte");
    assert.match(html, /<p[^>]*class="problem[^"]*"[^>]*role="alert"[^>]*>Open the sign-in address that iris serve printed\.<\/p>|<p[^>]*role="alert"[^>]*class="problem[^"]*"[^>]*>Open the sign-in address/);
    access.problem = "";
    assert.doesNotMatch(await render("/src/App.svelte"), /class="problem/);
  } finally {
    globalThis.fetch = saved;
    access.problem = "";
  }
});

// Card R6-05: on a phone, the first four views are tabs, and More opens the others.
// test/phone.test.mjs checks the layout in a browser.
test("the app has the phone tabs and a More popover with the other views", async () => {
  const html = await render("/src/App.svelte");
  const tabs = html.match(/<nav class="tabs[^"]*" aria-label="Main views">([\s\S]*?)<\/nav>/)?.[1] ?? "";
  assert.deepEqual([...tabs.matchAll(/href="([^"]+)"/g)].map((m) => m[1]), ["#/brief", "#/matters", "#/ask", "#/review"]);
  assert.match(tabs, /<button[^>]*popovertarget="more"[^>]*>[\s\S]*More/);
  const more = html.match(/<div id="more" popover[^>]*>([\s\S]*?)<\/div>/)?.[1] ?? "";
  assert.deepEqual([...more.matchAll(/href="([^"]+)"/g)].map((m) => m[1]), ["#/write", "#/outbox", "#/history", "#/jobs", "#/commitments", "#/conversations", "#/records", "#/feed", "#/signals"]);
});

// Card R6-06: the Mac app sets data-shell="mac". The window buttons sit at the left of the top bar,
// and the empty part of the bar drags the window.
test("the top bar makes room for the window buttons of the Mac app", async () => {
  const html = await render("/src/App.svelte");
  assert.match(html, /<header[^>]*data-tauri-drag-region/);
  const { readFile } = await import("node:fs/promises");
  const source = await readFile(new URL("../src/App.svelte", import.meta.url), "utf8");
  assert.match(source, /:global\(:root\[data-shell="mac"\]\) header \{\s*padding-left: calc\(var\(--space-16\) \+ var\(--space-8\)\);/);
  assert.match(source, /:global\(:root\[data-shell="mac"\]\) \.name \{\s*display: none;/);
});
