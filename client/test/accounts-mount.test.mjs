// More than one account (card R6-02): the short name of each account, the
// name on each brief line, and the accounts view.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount } from "./mount.mjs";

const fx = await load("/dev/fixtures.ts");
const { accountNames } = await load("/src/format.ts");
const { access } = await load("/src/access.svelte.ts");
const text = (el) => el.textContent.replace(/\s+/g, " ").trim();
const acc = (id, address) => ({ id, address });

test("each account gets the first short name that tells the accounts apart", () => {
  assert.deepEqual(accountNames([acc(1, "me@gmail.com")]), {}, "one account needs no name");
  assert.deepEqual(accountNames([acc(1, "me@gmail.com"), acc(2, "me@northwind.example")]), { "a-1": "gmail", "a-2": "northwind" });
  assert.deepEqual(accountNames([acc(1, "Dana@gmail.com"), acc(3, "work@gmail.com")]), { "a-1": "dana", "a-3": "work" });
  assert.deepEqual(accountNames([acc(1, "me@a.example"), acc(2, "me@a.org")]), { "a-1": "me@a.example", "a-2": "me@a.org" });
  // Property: for any list of different addresses, the names are different too.
  for (let n = 0; n < 500; n++) {
    const parts = ["me", "you", "dk", "a.b"];
    const domains = ["gmail.com", "gmail.org", "work.example", "a.b.c"];
    const seen = new Set();
    const list = [];
    for (let i = 0; i < 2 + (n % 4); i++) {
      const address = `${parts[(n * 7 + i * 3) % 4]}@${domains[(n + i * 5) % 4]}`;
      if (!seen.has(address)) (seen.add(address), list.push(acc(i + 1, address)));
    }
    const names = Object.values(accountNames(list));
    if (list.length > 1) assert.equal(new Set(names).size, list.length, JSON.stringify(list));
  }
});

test("a brief line names its account only when there are two or more", async () => {
  const item = fx.items.find((i) => i.account === "a-2");
  for (const [accounts, shown] of [[{}, null], [{ "a-1": "gmail", "a-2": "northwind" }, "northwind"]]) {
    access.accounts = accounts;
    const view = await mount("/src/views/Line.svelte", { item });
    try {
      const name = view.target.querySelector(".pill");
      assert.equal(name ? text(name) : null, shown);
      // The pill is outside the detail line, so a clamp of the detail never hides it.
      if (name) assert.ok(!name.closest(".detail") && name.nextElementSibling?.classList.contains("source"));
    } finally {
      view.destroy();
    }
  }
  access.accounts = {};
});

test("the accounts view lists each account with its state and how to add one", async () => {
  const stopped = { ...fx.status.accounts[1], state: "needs_sign_in" };
  const status = { ...fx.status, accounts: [fx.status.accounts[0], stopped] };
  const view = await mount("/src/views/Accounts.svelte", { status, poll: 0 });
  try {
    const rows = [...view.target.querySelectorAll(".accounts li")].map(text);
    assert.equal(rows.length, 2);
    assert.match(rows[0], /you@example\.com.*Shown as “example” in the brief.*Up to date/);
    assert.match(rows[1], /you@northwind\.example.*Google refused the sign-in.*iris account add client\.json/);
    assert.match(text(view.target), /Add an account.*iris account add client\.json/);
  } finally {
    view.destroy();
  }
});
