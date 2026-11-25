// More than one account (card R6-02): the short name of each account, the
// name on each brief line, and the accounts view.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

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

  // No status either: the result is unknown. The line says so and offers a check, and no Remove.
  status = { ok: false };
  view = await mount("/src/views/Accounts.svelte", { status: fx.status, poll: 0 });
  try {
    view.target.querySelector(`button[aria-label="Remove ${one.address}"]`).click();
    await tick();
    press(view, "Remove");
    await settle();
    const row = () => text(view.target.querySelector(".accounts li"));
    assert.match(row(), /Iris could not tell if this account is removed\. The removal can still run\./);
    assert.equal(view.target.querySelector(".ask"), null);
    assert.equal(view.target.querySelector(`button[aria-label="Remove ${one.address}"]`), null);
    // The engine still waits for a lock: the status gives the account as before. The doubt stays.
    status = { ok: true, body: fx.status };
    press(view, "Check again");
    await settle();
    assert.match(row(), /Iris could not tell if this account is removed\./);
    assert.equal(view.target.querySelector(`button[aria-label="Remove ${one.address}"]`), null);
    // The removal commits: the line says that Iris deletes the mail, then the account goes.
    status = { ok: true, body: { ...fx.status, accounts: [{ ...one, state: "removing" }, two] } };
    press(view, "Check again");
    await settle();
    assert.match(row(), /Iris deletes the mail of this account from this machine\./);
    assert.doesNotMatch(row(), /could not tell/);
  } finally {
    view.destroy();
    globalThis.fetch = saved;
  }
});

test("a polled status closes the question of an account in a removal, and shows a removed account", async () => {
  const saved = globalThis.fetch;
  const [one, two] = fx.status.accounts;
  let next = fx.status;
  globalThis.fetch = async () => new Response(JSON.stringify(next), { status: 200, headers: { "content-type": "application/json" } });
  const view = await mount("/src/views/Accounts.svelte", { status: fx.status, poll: 20 });
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  try {
    view.target.querySelector(`button[aria-label="Remove ${one.address}"]`).click();
    await wait(0);
    assert.notEqual(view.target.querySelector(".ask"), null);
    // The command removes the account: the poll gives it in state removing.
    next = { ...fx.status, accounts: [{ ...one, state: "removing" }, two] };
    await wait(80);
    assert.equal(view.target.querySelector(".ask"), null);
    assert.equal([...view.target.querySelectorAll("button")].some((b) => b.textContent.trim() === "Remove"), true, "the other account keeps its Remove");
    assert.equal(view.target.querySelector(`button[aria-label="Remove ${one.address}"]`), null);

    // The question of the other account, then the delete of the first ends: the other question stays.
    view.target.querySelector(`button[aria-label="Remove ${two.address}"]`).click();
    await wait(0);
    next = { ...fx.status, accounts: [two] };
    await wait(80);
    assert.notEqual(view.target.querySelector(".ask"), null);
    // The second account goes while its question is open: the page says that it is removed.
    next = { ...fx.status, accounts: [] };
    await wait(80);
    assert.equal(view.target.querySelector(".ask"), null);
    assert.match(text(view.target), new RegExp(`Removed ${two.address.replace(".", "\\.")}\\.`));
  } finally {
    view.destroy();
    globalThis.fetch = saved;
  }
});

test("a removal that answers removing shows the removed line when a later status no longer lists the account", async () => {
  const saved = globalThis.fetch;
  const [one, two] = fx.status.accounts;
  const json = (body) => new Response(JSON.stringify(body), { status: 200, headers: { "content-type": "application/json" } });
  let polled = fx.status;
  globalThis.fetch = async (url, init) =>
    init?.method === "POST" ? json({ ...fx.status, accounts: [{ ...one, state: "removing" }, two] }) : json(polled);
  const view = await mount("/src/views/Accounts.svelte", { status: fx.status, poll: 20 });
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  const press = (label) => [...view.target.querySelectorAll("button")].find((b) => b.textContent.trim() === label).click();
  try {
    polled = { ...fx.status, accounts: [{ ...one, state: "removing" }, two] };
    view.target.querySelector(`button[aria-label="Remove ${one.address}"]`).click();
    await wait(0);
    press("Remove");
    await wait(10);
    assert.equal(view.target.querySelector(".ask"), null);
    assert.match(text(view.target.querySelector(".accounts li")), /Iris deletes the mail of this account from this machine\./);
    assert.doesNotMatch(text(view.target), /Removed /);
    // The engine ends the delete: the next poll no longer lists the account.
    polled = { ...fx.status, accounts: [two] };
    await wait(80);
    assert.match(text(view.target), new RegExp(`Removed ${one.address.replace(".", "\\.")}\\.`));
  } finally {
    view.destroy();
    globalThis.fetch = saved;
  }
});

test("a removal answer that a newer poll hides never says that the account is removed (issue 932)", async () => {
  const saved = globalThis.fetch;
  const [one, two] = fx.status.accounts;
  const json = (body) => new Response(JSON.stringify(body), { status: 200, headers: { "content-type": "application/json" } });
  let release;
  let fail = false;
  globalThis.fetch = async (url, init) => {
    if (init?.method === "POST") {
      // The engine took the removal and answers while the delete runs, but only after a newer poll answered.
      await new Promise((r) => (release = r));
      return json({ ...fx.status, accounts: [{ ...one, state: "removing" }, two] });
    }
    if (fail) throw new TypeError("Failed to fetch");
    return json(fx.status);
  };
  const view = await mount("/src/views/Accounts.svelte", { status: fx.status, poll: 0 });
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  const press = (label) => [...view.target.querySelectorAll("button")].find((b) => b.textContent.trim() === label).click();
  try {
    view.target.querySelector(`button[aria-label="Remove ${one.address}"]`).click();
    await wait(0);
    press("Remove");
    await wait(5);
    // A read that started after the removal answers first, with the status from before it.
    await get("status");
    fail = true;
    release();
    await wait(10);
    const row = text(view.target.querySelector(".accounts li"));
    assert.doesNotMatch(row, /Iris removed this account/);
    assert.doesNotMatch(text(view.target), /Removed /);
    assert.match(row, /Iris removes this account and deletes its mail from this machine\./);
    assert.ok([...view.target.querySelectorAll("button")].some((b) => b.textContent.trim() === "Check again"));
  } finally {
    view.destroy();
    globalThis.fetch = saved;
  }
});

test("a slow poll that started before a removal does not put the removed account back", async () => {
  const saved = globalThis.fetch;
  const [one, two] = fx.status.accounts;
  const json = (body) => new Response(JSON.stringify(body), { status: 200, headers: { "content-type": "application/json" } });
  let release;
  let polls = 0;
  globalThis.fetch = async (url, init) => {
    if (init?.method === "POST") return json({ ...fx.status, accounts: [two] });
    polls += 1;
    // The first poll reads the old status and answers only after the removal.
    if (polls === 1) {
      await new Promise((r) => (release = r));
      return json(fx.status);
    }
    return json({ ...fx.status, accounts: [two] });
  };
  const view = await mount("/src/views/Accounts.svelte", { status: fx.status, poll: 10 });
  const wait = (ms) => new Promise((r) => setTimeout(r, ms));
  const press = (label) => [...view.target.querySelectorAll("button")].find((b) => b.textContent.trim() === label).click();
  try {
    await wait(30);
    assert.equal(polls, 1, "the first poll waits");
    view.target.querySelector(`button[aria-label="Remove ${one.address}"]`).click();
    await wait(0);
    press("Remove");
    await wait(10);
    assert.match(text(view.target), new RegExp(`Removed ${one.address.replace(".", "\\.")}\\.`));
    release();
    await wait(5);
    assert.equal(view.target.querySelectorAll(".accounts li").length, 1);
    assert.equal(view.target.querySelector(`button[aria-label="Remove ${one.address}"]`), null);
    // The shared access state (the account pills, the sign-in notice) does not take the old answer either.
    assert.equal(`a-${one.id}` in access.accounts, false, JSON.stringify(access.accounts));
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

// Issue 891: the person sets the time zone of the owner on the accounts page.
test("an engine with no time zone setting shows no time zone part", async () => {
  const { time_zone_set, ...old } = fx.status;
  const view = await mount("/src/views/Accounts.svelte", { status: old, poll: 0 });
  try {
    assert.equal(view.target.querySelector("#zone-pick"), null);
  } finally {
    view.destroy();
  }
});

test("the person sets the time zone, and sets it back to the zone of the machine", async () => {
  const saved = globalThis.fetch;
  const asked = [];
  let now = { time_zone: "Europe/London", time_zone_set: false };
  const json = (v) => new Response(JSON.stringify(v), { status: 200, headers: { "content-type": "application/json" } });
  globalThis.fetch = async (url, init) => {
    if (init?.method === "POST") {
      const { zone } = JSON.parse(init.body);
      asked.push([String(url), zone]);
      now = { time_zone: zone ?? "Europe/London", time_zone_set: zone !== null };
      return json(now);
    }
    return json({ ...fx.status, ...now });
  };
  const view = await mount("/src/views/Accounts.svelte", { status: { ...fx.status, ...now }, poll: 0 });
  const tick = async () => { for (let i = 0; i < 5; i++) await new Promise((r) => setTimeout(r, 0)); };
  const part = () => text(view.target.querySelector("section.zone"));
  const pick = view.target.querySelector("#zone-pick");
  const save = () => view.target.querySelector("section.zone button[type=submit]");
  const choose = (value) => ((pick.value = value), pick.dispatchEvent(new Event("change", { bubbles: true })));
  try {
    assert.match(part(), /Now: Europe\/London, the time zone of the machine that runs Iris\./);
    assert.equal(pick.value, "");
    assert.ok(save().disabled, "nothing to save before a change");

    choose("Europe/Berlin");
    await tick();
    assert.ok(!save().disabled);
    save().click();
    await tick();
    assert.deepEqual(asked, [["/api/zone", "Europe/Berlin"]]);
    assert.match(part(), /Now: Europe\/Berlin, set by you\. .*Saved\./);
    assert.equal(access.zone, "Europe/Berlin", "the days of the other views follow the new zone");

    choose("");
    await tick();
    save().click();
    await tick();
    assert.deepEqual(asked[1], ["/api/zone", null]);
    assert.match(part(), /Now: Europe\/London, the time zone of the machine that runs Iris\./);
  } finally {
    view.destroy();
    globalThis.fetch = saved;
    access.zone = null;
  }
});

test("a refused time zone shows the reason, and a lost answer says that Iris cannot tell", async () => {
  const saved = globalThis.fetch;
  let mode = "refuse";
  globalThis.fetch = async (url, init) => {
    if (init?.method === "POST" && mode === "refuse")
      return new Response(JSON.stringify({ error: 'not a time zone: "Europe/Berlin"' }), { status: 400, headers: { "content-type": "application/json" } });
    if (init?.method === "POST") throw new TypeError("Failed to fetch");
    return new Response(JSON.stringify(fx.status), { status: 200, headers: { "content-type": "application/json" } });
  };
  const view = await mount("/src/views/Accounts.svelte", { status: fx.status, poll: 0 });
  const tick = async () => { for (let i = 0; i < 5; i++) await new Promise((r) => setTimeout(r, 0)); };
  const pick = view.target.querySelector("#zone-pick");
  const save = () => view.target.querySelector("section.zone button[type=submit]");
  try {
    pick.value = "Europe/Berlin";
    pick.dispatchEvent(new Event("change", { bubbles: true }));
    await tick();
    save().click();
    await tick();
    assert.match(text(view.target.querySelector("section.zone [role=alert]")), /not a time zone/);
    mode = "lost";
    save().click();
    await tick();
    assert.match(text(view.target.querySelector("section.zone [role=alert]")), /Iris could not tell if it saved the time zone\./);
    assert.doesNotMatch(text(view.target), /Saved\./);
  } finally {
    view.destroy();
    globalThis.fetch = saved;
  }
});

// Review of PR 1009. Invariant: after a save that the engine took, the shared zone of the other views
// (`access.zone`) is the saved zone, whatever the status reads around it do.
test("a saved zone sets the shared zone, also when the next status read fails or an older read answers late", async () => {
  const saved = globalThis.fetch;
  const { readStatus } = await load("/src/api.ts");
  const json = (v) => new Response(JSON.stringify(v), { status: 200, headers: { "content-type": "application/json" } });
  let late;
  const old = { ...fx.status, time_zone: "Europe/London", time_zone_set: false };
  let reads = 0;
  globalThis.fetch = async (url, init) => {
    if (init?.method === "POST") return json({ time_zone: JSON.parse(init.body).zone, time_zone_set: true });
    reads++;
    // The first read is slow and old: it answers after the save. Each later read fails.
    if (reads === 1) return new Promise((r) => (late = () => r(json(old))));
    throw new TypeError("Failed to fetch");
  };
  const view = await mount("/src/views/Accounts.svelte", { status: old, poll: 0 });
  const tick = async () => { for (let i = 0; i < 5; i++) await new Promise((r) => setTimeout(r, 0)); };
  const pick = view.target.querySelector("#zone-pick");
  try {
    const slow = readStatus().catch(() => null);
    pick.value = "UTC";
    pick.dispatchEvent(new Event("change", { bubbles: true }));
    await tick();
    view.target.querySelector("section.zone button[type=submit]").click();
    await tick();
    assert.equal(access.zone, "UTC", "the failed read after the save keeps the saved zone");
    assert.match(text(view.target.querySelector("section.zone")), /Now: UTC, set by you\..*Saved\./);
    late();
    await slow;
    await tick();
    assert.equal(access.zone, "UTC", "an older read that answers late does not put the old zone back");
  } finally {
    view.destroy();
    globalThis.fetch = saved;
    access.zone = null;
  }
});

test("UTC is a choice, and a machine zone with no name is not called UTC", async () => {
  const view = await mount("/src/views/Accounts.svelte", { status: { ...fx.status, time_zone: null, time_zone_set: false }, poll: 0 });
  try {
    const part = text(view.target.querySelector("section.zone"));
    assert.match(part, /Now: the time zone of the machine that runs Iris\. Iris cannot read the name of this zone\./);
    assert.doesNotMatch(part, /Now: UTC/);
    const values = [...view.target.querySelectorAll("#zone-pick option")].map((o) => o.value);
    assert.equal(values.filter((v) => v === "UTC").length, 1);
    assert.ok(values.includes("Europe/Berlin"));
  } finally {
    view.destroy();
  }
});

test("the time zone selector follows a status change from outside the page, and keeps an unsaved choice (issue 1016)", async () => {
  const saved = globalThis.fetch;
  let now = { time_zone: "Europe/London", time_zone_set: false };
  globalThis.fetch = async () => new Response(JSON.stringify({ ...fx.status, ...now }), { status: 200, headers: { "content-type": "application/json" } });
  const view = await mount("/src/views/Accounts.svelte", { status: { ...fx.status, ...now }, poll: 10 });
  const pick = view.target.querySelector("#zone-pick");
  const save = () => view.target.querySelector("section.zone button[type=submit]");
  try {
    assert.equal(pick.value, "");
    // Another tab or `iris zone` sets the zone: the next poll shows it in the selector.
    now = { time_zone: "Asia/Tokyo", time_zone_set: true };
    await until(() => (view.flushSync(), /Now: Asia\/Tokyo, set by you/.test(text(view.target.querySelector("section.zone")))));
    assert.equal(pick.value, "Asia/Tokyo");
    assert.ok(save().disabled, "nothing to save: the selector shows the zone now");
    // An unsaved choice stays through a poll that shows another change.
    pick.value = "Europe/Berlin";
    pick.dispatchEvent(new Event("change", { bubbles: true }));
    now = { time_zone: "UTC", time_zone_set: true };
    await until(() => (view.flushSync(), /Now: UTC, set by you/.test(text(view.target.querySelector("section.zone")))));
    assert.equal(pick.value, "Europe/Berlin");
    assert.ok(!save().disabled);
  } finally {
    view.destroy();
    globalThis.fetch = saved;
  }
});
