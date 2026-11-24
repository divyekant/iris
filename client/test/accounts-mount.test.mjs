// More than one account (card R6-02): the short name of each account, the
// name on each brief line, and the accounts view.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount } from "./mount.mjs";

const fx = await load("/dev/fixtures.ts");
const { accountNames } = await load("/src/format.ts");
const { access } = await load("/src/access.svelte.ts");
const { get } = await load("/src/api.ts");
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

test("a sync whose process stopped says so, for the first copy and for a later sync", async () => {
  const stopped = { kind: "first", started: "2026-11-23T10:00:00Z", ended: null, result: "stopped: the process ended before the sync ended" };
  const first = { ...fx.status.accounts[0], mirror_ready_at: null, sync: { kind: "first", step: 4 }, last_sync: stopped };
  const later = { ...fx.status.accounts[1], state: "active", last_sync: { ...stopped, kind: "incremental" } };
  const view = await mount("/src/views/Accounts.svelte", { status: { ...fx.status, accounts: [first, later] }, poll: 0 });
  try {
    const rows = [...view.target.querySelectorAll(".accounts li")].map(text);
    assert.match(rows[0], /The last try of the first copy stopped before it ended\. Iris tries again each minute\./);
    assert.match(rows[1], /The last sync stopped before it ended\. Iris tries again each minute\./);
    for (const row of rows) assert.doesNotMatch(row, /copies this mailbox|Up to date/);
    assert.equal(view.target.querySelectorAll(".tone.retry").length, 2);
  } finally {
    view.destroy();
  }
});

test("a read-only account says that its approved changes wait, and names the command that gives write access", async () => {
  const reader = { ...fx.status.accounts[0], state: "read_only" };
  const view = await mount("/src/views/Accounts.svelte", { status: { ...fx.status, accounts: [reader] }, poll: 0 });
  try {
    const row = text(view.target.querySelector(".accounts li"));
    assert.match(row, /Iris can read this mailbox, but it cannot change it\. Your approved changes wait\./);
    assert.match(row, /To give write access, run iris account add client\.json --write on the machine that runs Iris and sign in again\./);
    // The copy still runs: the sync state stays, and the tone is not a plain "ok".
    assert.match(row, /Up to date/);
    assert.ok(view.target.querySelector(".tone.retry"));
  } finally {
    view.destroy();
  }
});

test("a read-only account with a failed sync shows the failure and the read-only words", async () => {
  const reader = { ...fx.status.accounts[0], state: "read_only", last_sync: { ...fx.status.accounts[0].last_sync, result: "error: HTTP 503" } };
  const view = await mount("/src/views/Accounts.svelte", { status: { ...fx.status, accounts: [reader] }, poll: 0 });
  try {
    const row = text(view.target.querySelector(".accounts li"));
    assert.match(row, /The last sync failed\. Iris tries again each minute\./);
    assert.match(row, /Iris can read this mailbox, but it cannot change it\./);
    assert.match(row, /iris account add client\.json --write/);
  } finally {
    view.destroy();
  }
});

test("a stopped IMAP account names the mail server and the IMAP sign-in command, which needs only the address", async () => {
  const stopped = { ...fx.status.accounts[1], provider: "imap", state: "needs_sign_in" };
  const view = await mount("/src/views/Accounts.svelte", { status: { ...fx.status, accounts: [stopped] }, poll: 0 });
  try {
    const row = text(view.target.querySelector(".accounts li"));
    assert.match(row, /The mail server refused the sign-in/);
    assert.match(row, /run iris account add-imap you@northwind\.example and give the new password/);
    assert.doesNotMatch(row, /Google|client\.json|--host/);
  } finally {
    view.destroy();
  }
});

test("the IMAP sign-in help ends the options before an address that starts with a dash", async () => {
  const { signIn } = await load("/src/format.ts");
  assert.equal(signIn({ provider: "imap", address: "-x@example.org" }).command, "iris account add-imap -- -x@example.org");
  // Each other address keeps the command as it was.
  assert.equal(signIn({ provider: "imap", address: "you@northwind.example" }).command, "iris account add-imap you@northwind.example");
});

test("each shell argument of a sign-in command keeps its exact text in a POSIX shell", async () => {
  const { shellArg } = await load("/src/format.ts");
  const { execFileSync } = await import("node:child_process");
  for (const s of ["you@example.com", "o'brien@example.com", "a b@example.com", "x\\y@example.com", "$(id)@example.com", "-n"]) {
    const out = execFileSync("sh", ["-c", `printf %s ${shellArg(s)}`]).toString();
    assert.equal(out, s);
  }
});

test("a try that runs after a failed try says that the last try failed", async () => {
  const running = { kind: "incremental", started: "2026-11-23T10:01:00Z", ended: null, result: null, after_failure: true };
  const first = { ...fx.status.accounts[0], mirror_ready_at: null, sync: { kind: "first", step: 4 }, last_sync: { ...running, kind: "first" } };
  const later = { ...fx.status.accounts[1], state: "active", last_sync: running };
  // The same running try after a good try, and from an older engine with no field.
  const good = { ...fx.status.accounts[1], id: 9, state: "active", last_sync: { ...running, after_failure: false } };
  const { after_failure, ...old } = running;
  const older = { ...fx.status.accounts[1], id: 10, state: "active", last_sync: old };
  const view = await mount("/src/views/Accounts.svelte", { status: { ...fx.status, accounts: [first, later, good, older] }, poll: 0 });
  try {
    const rows = [...view.target.querySelectorAll(".accounts li")].map(text);
    for (const row of rows.slice(0, 2)) {
      assert.match(row, /The last try failed\. Iris tries again now\./);
      assert.doesNotMatch(row, /copies this mailbox|Up to date/);
    }
    for (const row of rows.slice(2)) assert.match(row, /Up to date\./);
    assert.equal(view.target.querySelectorAll(".tone.retry").length, 2);
  } finally {
    view.destroy();
  }
});

test("remove asks one question in place, Keep changes nothing, and Remove posts the address", async () => {
  const saved = globalThis.fetch;
  const asked = [];
  const [one, two] = fx.status.accounts;
  globalThis.fetch = async (url, init) => {
    asked.push([String(url), JSON.parse(init.body)]);
    return new Response(JSON.stringify({ ...fx.status, accounts: [two] }), { status: 200, headers: { "content-type": "application/json" } });
  };
  const view = await mount("/src/views/Accounts.svelte", { status: fx.status, poll: 0 });
  const tick = () => new Promise((r) => setTimeout(r, 0));
  const press = (label) => [...view.target.querySelectorAll("button")].find((b) => b.textContent.trim() === label).click();
  try {
    view.target.querySelector(`button[aria-label="Remove ${one.address}"]`).click();
    await tick();
    assert.match(text(view.target), new RegExp(`Remove ${one.address.replace(".", "\\.")}\\? Iris deletes its mail from this machine\\.`));
    assert.match(text(view.target), /The files of agents in its jobs go\. A backup and the notes of jobs keep their copy\./);
    assert.match(text(view.target), /The record of what Iris and you did stays: drafts, the reasons of changes, job events and the titles of matters that you decided can hold text from this mail\./);
    press("Keep");
    await tick();
    assert.doesNotMatch(text(view.target), /Iris deletes its mail/);
    assert.equal(asked.length, 0);

    view.target.querySelector(`button[aria-label="Remove ${one.address}"]`).click();
    await tick();
    press("Remove");
    for (let i = 0; i < 5; i++) await tick();
    assert.deepEqual(asked, [[`/api/accounts/${one.id}/remove`, { address: one.address }]]);
    assert.match(text(view.target), new RegExp(`Removed ${one.address.replace(".", "\\.")}\\. Its mail is gone from Iris on this machine\\.`));
    assert.equal(view.target.querySelectorAll(".accounts li").length, 1);
  } finally {
    view.destroy();
    globalThis.fetch = saved;
  }
});

test("a remove whose answer is lost shows what a fresh status says, and an unknown result offers a check", async () => {
  const saved = globalThis.fetch;
  const [one, two] = fx.status.accounts;
  const json = (body) => new Response(JSON.stringify(body), { status: 200, headers: { "content-type": "application/json" } });
  const tick = () => new Promise((r) => setTimeout(r, 0));
  const settle = async () => {
    for (let i = 0; i < 8; i++) await tick();
  };
  const press = (view, label) => [...view.target.querySelectorAll("button")].find((b) => b.textContent.trim() === label).click();
  // The engine removed the account, but the answer was lost: the status has no line for it.
  let status = { ok: true, body: { ...fx.status, accounts: [two] } };
  globalThis.fetch = async (url, init) => {
    if (init?.method === "POST") throw new TypeError("Failed to fetch");
    if (!status.ok) throw new TypeError("Failed to fetch");
    return json(status.body);
  };
  let view = await mount("/src/views/Accounts.svelte", { status: fx.status, poll: 0 });
  try {
    view.target.querySelector(`button[aria-label="Remove ${one.address}"]`).click();
    await tick();
    press(view, "Remove");
    await settle();
    assert.match(text(view.target), new RegExp(`Removed ${one.address.replace(".", "\\.")}\\.`));
    assert.doesNotMatch(text(view.target), /Failed to fetch/);
    assert.equal(view.target.querySelectorAll(".accounts li").length, 1);
  } finally {
    view.destroy();
  }

  // No status either: the result is unknown, and the question offers a check in place of Remove.
  status = { ok: false };
  view = await mount("/src/views/Accounts.svelte", { status: fx.status, poll: 0 });
  try {
    view.target.querySelector(`button[aria-label="Remove ${one.address}"]`).click();
    await tick();
    press(view, "Remove");
    await settle();
    assert.match(text(view.target), /Iris could not tell if the account is removed\. Check again before you remove it again\./);
    assert.equal([...view.target.querySelectorAll(".ask button")].some((b) => b.textContent.trim() === "Remove"), false);
    // The check reads a status where the engine still deletes: the question closes, the line says so.
    status = { ok: true, body: { ...fx.status, accounts: [{ ...one, state: "removing" }, two] } };
    press(view, "Check again");
    await settle();
    assert.equal(view.target.querySelector(".ask"), null);
    assert.match(text(view.target.querySelector(".accounts li")), /Iris deletes the mail of this account from this machine\./);
  } finally {
    view.destroy();
    globalThis.fetch = saved;
  }
});

test("an account in state removing says that Iris deletes its mail and offers no remove", async () => {
  const going = { ...fx.status.accounts[0], state: "removing", mirror_ready_at: null, sync: null };
  const view = await mount("/src/views/Accounts.svelte", { status: { ...fx.status, accounts: [going] }, poll: 0 });
  try {
    const row = text(view.target.querySelector(".accounts li"));
    assert.match(row, /Iris deletes the mail of this account from this machine\./);
    assert.doesNotMatch(row, /copies this mailbox/);
    assert.equal(view.target.querySelector("button.remove"), null);
  } finally {
    view.destroy();
  }
});
