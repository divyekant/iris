// The app shell shows the Iris name and the navigation to each view.
import { test } from "node:test";
import assert from "node:assert/strict";
import { render } from "./render.mjs";

test("the app shows the Iris name and the views", async () => {
  const html = await render("/src/App.svelte");
  assert.match(html, /<a class="name[^"]*" href="#\/">Iris<\/a>/);
  for (const view of ["Brief", "Commitments", "Conversations", "Records", "Feed", "Signals", "Measures"]) {
    assert.match(html, new RegExp(`>${view}</a>`));
  }
});
