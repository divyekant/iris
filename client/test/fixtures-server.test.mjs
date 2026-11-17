// The fixture server of `pnpm dev --mode fixtures`: a write that no fixture route handles must not get a read as its answer.
import { after, test } from "node:test";
import assert from "node:assert/strict";
import { createServer } from "vite";

const vite = await createServer({
  root: new URL("..", import.meta.url).pathname,
  mode: "fixtures",
  // Its own cache: the other test files start Vite at the same time.
  cacheDir: "node_modules/.vite-fixtures",
  server: { port: 0, hmr: false },
  logLevel: "error",
});
after(() => vite.close());
await vite.listen();
const base = vite.resolvedUrls.local[0];

test("a comment POST is refused, and the GET gives the comments", async () => {
  const got = await fetch(`${base}api/changesets/cs-79/comments`);
  assert.equal(got.status, 200);
  assert.ok(Array.isArray((await got.json()).comments));
  const sent = await fetch(`${base}api/changesets/cs-79/comments`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ revision: 1, text: "Keep it.", key: "k-0123456789abcdef" }) });
  assert.equal(sent.status, 405);
  assert.match((await sent.json()).error, /do not save/);
});
