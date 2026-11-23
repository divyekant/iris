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

test("the Ask route refuses a question over 500 characters, as the engine", async () => {
  const ask = (question) => fetch(`${base}api/ask`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ question }) });
  // The engine counts the characters (code points) of the trimmed question.
  const over = await ask("a".repeat(501));
  assert.equal(over.status, 400);
  assert.match((await over.json()).error, /1 to 500 characters/);
  // 500 emoji are 1,000 UTF-16 units but 500 characters, and spaces around them do not count.
  const most = await ask(`  ${"😀".repeat(500)}  `);
  assert.equal(most.status, 200);
  assert.equal((await ask("😀".repeat(501))).status, 400);
});
