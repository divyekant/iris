// Render App.svelte on the server and check that the page shows the Iris name.
import { test } from "node:test";
import assert from "node:assert/strict";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { compile } from "svelte/compiler";
import { render } from "svelte/server";

test("the app shows the Iris heading", async () => {
  const source = readFileSync(new URL("../src/App.svelte", import.meta.url), "utf8");
  const { js } = compile(source, { generate: "server", filename: "App.svelte" });
  // Write inside client/, so that the import of `svelte` resolves to node_modules.
  const out = new URL("../.test-build/App.server.js", import.meta.url);
  mkdirSync(new URL(".", out), { recursive: true });
  writeFileSync(out, js.code);
  const { default: App } = await import(out.href);
  assert.match(render(App).body, /<h1[^>]*>Iris<\/h1>/);
});
