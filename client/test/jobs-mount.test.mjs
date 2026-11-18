// The Jobs view in a DOM: "Show more" asks for the same page size as the first
// request of each state (issue #329).
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

const fx = await load("/dev/fixtures.ts");
const { pageSize } = await load("/src/jobs.ts");

test("Show more asks for 20 ended jobs and 50 jobs of another state, the size of the first request", async () => {
  const asked = [];
  globalThis.fetch = async (url) => {
    asked.push(String(url));
    return new Response(JSON.stringify({ jobs: [], next: null }), { status: 200, headers: { "content-type": "application/json" } });
  };
  const page = (job) => ({ jobs: [job], next: "c1" });
  const pages = { needs_you: { jobs: [], next: null }, working: page(fx.jobs["j-15"]), waiting: { jobs: [], next: null }, done: page(fx.jobs["j-9"]) };
  const view = await mount("/src/views/Jobs.svelte", { pages });
  const buttons = () => [...view.target.querySelectorAll("button")].filter((b) => b.textContent.includes("Show more"));
  assert.equal(buttons().length, 2);
  buttons()[1].click();
  await until(() => asked.length === 1);
  assert.match(asked[0], /state=done&limit=20&after=c1$/);
  await until(() => buttons().length === 1);
  buttons()[0].click();
  await until(() => asked.length === 2);
  assert.match(asked[1], /state=working&limit=50&after=c1$/);
  assert.equal(pageSize("done"), 20);
  for (const state of ["needs_you", "working", "waiting"]) assert.equal(pageSize(state), 50);
  view.destroy();
});
