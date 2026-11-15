import { test } from "node:test";
import assert from "node:assert/strict";
import { hello } from "../src/index.mjs";

test("hello names the person", () => {
  assert.equal(hello("dk"), "Hello, dk.");
});
