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
  access.problem = "Open the sign-in address that iris serve printed.";
  try {
    const html = await render("/src/App.svelte");
    assert.match(html, /<p[^>]*class="problem[^"]*"[^>]*role="alert"[^>]*>Open the sign-in address that iris serve printed\.<\/p>|<p[^>]*role="alert"[^>]*class="problem[^"]*"[^>]*>Open the sign-in address/);
  } finally {
    access.problem = "";
  }
  assert.doesNotMatch(await render("/src/App.svelte"), /class="problem/);
});
