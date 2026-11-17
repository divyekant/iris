// The write view (R3-04, docs/specs/2026-11-17-write.md, section 6) in a
// DOM: chips, the warnings before Send, one key for one content, the key
// after a lost answer and a reload, and the state of the write.
import { test, beforeEach } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

const route = await load("/src/route.ts");
const text = (el) => el.textContent.replace(/\s+/g, " ");
Object.defineProperty(globalThis, "sessionStorage", { configurable: true, value: window.sessionStorage });
beforeEach(() => window.sessionStorage.clear());

const ACCOUNTS = [{ id: 1, address: "dk@example.com", provider: "gmail", state: null, mirror_ready_at: null, sync: null, last_sync: null }];
const REPLY = {
  account: "a-1",
  parent: { id: "m-9", from: { name: "Priya Shah", address: "priya@example.com" }, date: "2026-11-17T09:12:00Z", text: "Could you send the final Q4 numbers by Thursday?" },
  to: ["priya@example.com"],
  cc: [],
  subject: "Re: Q4 numbers",
};

/** A fake engine. `answer(path, method, body)` gives [status, body], or throws for no answer. */
function serve(answer) {
  const asked = [];
  const saved = globalThis.fetch;
  globalThis.fetch = async (url, init = {}) => {
    const path = String(url).replace(/^\/api\//, "");
    const method = init.method ?? "GET";
    const body = init.body ? JSON.parse(init.body) : undefined;
    asked.push({ method, path, body });
    const [status, out] = answer(path, method, body);
    return new Response(JSON.stringify(out), { status, headers: { "content-type": "application/json" } });
  };
  return { asked, posts: () => asked.filter((a) => a.method === "POST"), restore: () => (globalThis.fetch = saved) };
}

function type(el, value) {
  el.value = value;
  el.dispatchEvent(new window.Event("input", { bubbles: true }));
}
const press = (app) => app.target.querySelector("form").dispatchEvent(new window.Event("submit", { bubbles: true, cancelable: true }));

async function fill(app, to = "sam@okafor.example ") {
  type(app.target.querySelector("#write-to"), to);
  app.flushSync();
  const subject = app.target.querySelector("input.subject");
  if (subject) type(subject, "Dinner on Saturday?");
  type(app.target.querySelector("textarea"), "Hi Sam,\n\nAre you free?\n");
  app.flushSync();
}

test("the routes of the write view", () => {
  assert.deepEqual(route.parse("#/write"), { view: "write", thread: null });
  assert.deepEqual(route.parse("#/write/"), { view: "write", thread: null });
  assert.deepEqual(route.parse("#/write/t-4"), { view: "write", thread: "t-4" });
  assert.deepEqual(route.parse("#/write/m-4"), { view: "home" });
  assert.deepEqual(route.parse("#/write/t-4/x"), { view: "home" });
  assert.equal(route.href.reply("t-4"), "#/write/t-4");
  assert.ok(route.NAV.some(([link]) => link === "#/write"));
});

test("a typed address becomes a chip, and Send waits for a valid one", async () => {
  const net = serve(() => [500, {}]);
  const app = await mount("/src/views/Write.svelte", { start: { accounts: ACCOUNTS } });
  try {
    const send = () => app.target.querySelector("button[type=submit]");
    assert.equal(send().disabled, true);
    type(app.target.querySelector("#write-to"), "not-an-address, sam@okafor.example ");
    app.flushSync();
    const chips = [...app.target.querySelectorAll(".chip")];
    assert.deepEqual(chips.map((c) => c.firstChild.textContent.trim()), ["not-an-address", "sam@okafor.example"]);
    assert.ok(chips[0].classList.contains("bad"));
    type(app.target.querySelector("input.subject"), "Hi");
    app.flushSync();
    assert.equal(send().disabled, false);
    chips[1].querySelector("button").click();
    app.flushSync();
    assert.equal(send().disabled, true, "only an address that is not valid is left");
  } finally {
    app.destroy();
    net.restore();
  }
});

test("a 409 shows the warnings; the next press sends them with the same key, then the state follows the engine", async () => {
  let reads = 0;
  const net = serve((path, method, body) => {
    if (method === "POST" && path === "write") {
      if (body.flags.length === 0) return [409, { error: "Look at the warnings, then press Send again.", flags: ["new_recipient"] }];
      return [201, { write: "cs-3" }];
    }
    if (path === "write/cs-3") {
      reads += 1;
      return [200, reads === 1 ? { state: "saving" } : { state: "waiting", draft: "d-7", sending: true, leaves_at: new Date(Date.now() + 50_000).toISOString() }];
    }
    return [404, { error: "Not found." }];
  });
  const app = await mount("/src/views/Write.svelte", { start: { accounts: ACCOUNTS }, every: 20 });
  try {
    await fill(app);
    press(app);
    await until(() => app.target.querySelector(".flags"));
    app.flushSync();
    assert.match(text(app.target), /Goes to an address you never wrote to/);
    assert.match(text(app.target), /Look at the warnings, then press Send again\./);
    press(app);
    await until(() => app.target.querySelector(".done h1")?.textContent.includes("On its way"));
    app.flushSync();
    const [first, second] = net.posts();
    assert.equal(first.body.key, second.body.key, "one content, one key");
    assert.match(first.body.key, /^[0-9a-f-]{36}$/);
    assert.deepEqual(second.body.flags, ["new_recipient"]);
    assert.deepEqual(
      { ...second.body, key: undefined, flags: undefined },
      { account: "a-1", subject: "Dinner on Saturday?", to: ["sam@okafor.example"], cc: [], body: "Hi Sam,\n\nAre you free?\n", key: undefined, flags: undefined },
    );
    assert.match(text(app.target), /On its way to sam@okafor\.example/);
    assert.match(text(app.target), /Leaves in 0:[45]\d/);
    assert.equal(window.sessionStorage.getItem("iris.write.new"), null, "the engine took the mail: the form is gone");
  } finally {
    app.destroy();
    net.restore();
  }
});

test("a change of the content makes a new key", async () => {
  const net = serve(() => [400, { error: "No." }]);
  const app = await mount("/src/views/Write.svelte", { start: { accounts: ACCOUNTS } });
  try {
    await fill(app);
    press(app);
    await until(() => net.posts().length === 1);
    type(app.target.querySelector("textarea"), "Hi Sam,\n\nAre you free on Sunday?\n");
    app.flushSync();
    press(app);
    await until(() => net.posts().length === 2);
    const [a, b] = net.posts();
    assert.notEqual(a.body.key, b.body.key);
  } finally {
    app.destroy();
    net.restore();
  }
});

test("after a lost answer and a reload, a press sends the first key", async () => {
  let lost = true;
  const saved = globalThis.fetch;
  const keys = [];
  globalThis.fetch = async (url, init = {}) => {
    const path = String(url).replace(/^\/api\//, "");
    if (init.method === "POST" && path === "write") {
      keys.push(JSON.parse(init.body).key);
      if (lost) throw new TypeError("network");
      return new Response(JSON.stringify({ write: "cs-3" }), { status: 200 });
    }
    return new Response(JSON.stringify({ state: "saving" }), { status: 200 });
  };
  let app = await mount("/src/views/Write.svelte", { start: { accounts: ACCOUNTS } });
  try {
    await fill(app);
    press(app);
    await until(() => app.target.querySelector("[role=alert]"));
    app.flushSync();
    assert.match(text(app.target), /Iris does not know if it took the mail\. Press Send again: Iris sends it only once\./);
    // The reload: a new view reads the form and its key from the storage.
    app.destroy();
    lost = false;
    app = await mount("/src/views/Write.svelte", { start: { accounts: ACCOUNTS } });
    assert.equal(app.target.querySelector("textarea").value, "Hi Sam,\n\nAre you free?\n");
    assert.ok(text(app.target).includes("sam@okafor.example"));
    press(app);
    await until(() => keys.length === 2);
    assert.equal(keys[1], keys[0]);
  } finally {
    app.destroy();
    globalThis.fetch = saved;
  }
});

test("a reply shows the mail that it answers and names the parent", async () => {
  const net = serve((path, method) => (method === "POST" ? [201, { write: "cs-4" }] : [200, { state: "stopped", reason: "You stopped this mail.", draft: null }]));
  const app = await mount("/src/views/Write.svelte", { start: { thread: "t-2", reply: REPLY }, every: 20 });
  try {
    assert.match(text(app.target.querySelector(".parent")), /Priya Shah/);
    assert.match(text(app.target.querySelector(".parent")), /Could you send the final Q4 numbers by Thursday\?/);
    assert.match(text(app.target), /Re: Q4 numbers/);
    assert.equal(app.target.querySelector("input.subject"), null, "Iris sets the subject of a reply");
    type(app.target.querySelector("textarea"), "Yes, on Wednesday.\n");
    app.flushSync();
    press(app);
    await until(() => app.target.querySelector(".done h1")?.textContent.includes("did not leave"));
    const [post] = net.posts();
    assert.deepEqual(post.body.to, ["priya@example.com"]);
    assert.equal(post.body.thread, "t-2");
    assert.equal(post.body.parent, "m-9");
    assert.equal("subject" in post.body, false);
    assert.equal("account" in post.body, false);
  } finally {
    app.destroy();
    net.restore();
  }
});

test("Stop in saving withdraws the write; a 409 says that the mail cannot stop", async () => {
  let refuse = false;
  const net = serve((path, method) => {
    if (method === "POST" && path === "write") return [201, { write: "cs-3" }];
    if (method === "POST" && path === "changesets/cs-3/withdraw") return refuse ? [409, { error: "No." }] : [200, {}];
    if (path === "write/cs-3") return [200, { state: "saving" }];
    return [404, { error: "Not found." }];
  });
  const app = await mount("/src/views/Write.svelte", { start: { accounts: ACCOUNTS }, every: 20 });
  try {
    await fill(app);
    press(app);
    const stop = () => [...app.target.querySelectorAll("button")].find((b) => b.textContent.includes("Stop this mail"));
    await until(() => stop());
    refuse = true;
    stop().click();
    await until(() => text(app.target).includes("Iris cannot stop it"));
    assert.match(text(app.target), /The mail left or is leaving now\. Iris cannot stop it\./);
    assert.ok(net.posts().some((p) => p.path === "changesets/cs-3/withdraw"));
  } finally {
    app.destroy();
    net.restore();
  }
});

test("a reload keeps an address that is not a chip yet, and the warnings that the next press sends", async () => {
  const net = serve((path, method, body) => {
    if (method === "POST" && path === "write") {
      return body.flags.length ? [201, { write: "cs-3" }] : [409, { error: "Look at the warnings, then press Send again.", flags: ["new_recipient"] }];
    }
    return [200, { state: "saving" }];
  });
  let app = await mount("/src/views/Write.svelte", { start: { accounts: ACCOUNTS } });
  try {
    type(app.target.querySelector("#write-to"), "sam@okafor.exa");
    app.flushSync();
    app.destroy();
    app = await mount("/src/views/Write.svelte", { start: { accounts: ACCOUNTS } });
    assert.equal(app.target.querySelector("#write-to").value, "sam@okafor.exa", "the open text of the field");
    type(app.target.querySelector("#write-to"), "sam@okafor.example ");
    app.flushSync();
    type(app.target.querySelector("input.subject"), "Dinner on Saturday?");
    type(app.target.querySelector("textarea"), "Hi Sam,\n");
    app.flushSync();
    press(app);
    await until(() => app.target.querySelector(".flags"));
    // The reload after the warnings: the next press sends them, with the same key.
    app.destroy();
    app = await mount("/src/views/Write.svelte", { start: { accounts: ACCOUNTS } });
    press(app);
    await until(() => net.posts().length === 2);
    const [first, second] = net.posts();
    assert.deepEqual(second.body.flags, ["new_recipient"]);
    assert.equal(second.body.key, first.body.key);
  } finally {
    app.destroy();
    net.restore();
  }
});

test("a late answer of a view that closed does not remove the newer form", async () => {
  let answer;
  const late = new Promise((resolve) => (answer = resolve));
  const saved = globalThis.fetch;
  let posts = 0;
  globalThis.fetch = async (url, init = {}) => {
    const path = String(url).replace(/^\/api\//, "");
    if (init.method === "POST" && path === "write" && ++posts === 1) {
      await late;
      return new Response(JSON.stringify({ write: "cs-3" }), { status: 201 });
    }
    return new Response(JSON.stringify({ state: "saving" }), { status: 200 });
  };
  let app = await mount("/src/views/Write.svelte", { start: { accounts: ACCOUNTS } });
  try {
    await fill(app);
    press(app);
    app.destroy();
    window.sessionStorage.clear();
    app = await mount("/src/views/Write.svelte", { start: { accounts: ACCOUNTS } });
    await fill(app, "ana@example.com ");
    const newer = JSON.parse(window.sessionStorage.getItem("iris.write.new"));
    answer();
    await new Promise((resolve) => setTimeout(resolve, 50));
    const kept = JSON.parse(window.sessionStorage.getItem("iris.write.new"));
    assert.equal(kept?.key, newer.key, "the newer form stays");
    assert.deepEqual(kept.to, ["ana@example.com"]);
  } finally {
    app.destroy();
    globalThis.fetch = saved;
  }
});

test("a long mail goes without keepalive, a short one with it", async () => {
  const saved = globalThis.fetch;
  const sent = [];
  globalThis.fetch = async (url, init = {}) => {
    if (init.method === "POST") sent.push({ size: new TextEncoder().encode(init.body).length, keepalive: init.keepalive });
    return new Response(JSON.stringify(init.method === "POST" ? { write: "cs-3" } : { state: "saving" }), { status: 201 });
  };
  for (const size of [100, 70 * 1024, 256 * 1024]) {
    window.sessionStorage.clear();
    const app = await mount("/src/views/Write.svelte", { start: { accounts: ACCOUNTS } });
    try {
      await fill(app);
      type(app.target.querySelector("textarea"), "x".repeat(size));
      app.flushSync();
      press(app);
      await until(() => sent.length && sent.at(-1).size > size);
    } finally {
      app.destroy();
    }
  }
  globalThis.fetch = saved;
  assert.deepEqual(sent.map((s) => s.keepalive), [true, false, false]);
});

test("a late state read of the write before Write another does not show", async () => {
  const gate = () => {
    let open;
    const p = new Promise((resolve) => (open = resolve));
    return { p, open };
  };
  const a = gate();
  const b = gate();
  let writes = 0;
  let aHeld = false;
  const net = serve(() => [500, {}]);
  globalThis.fetch = async (url, init = {}) => {
    const path = String(url).replace(/^\/api\//, "");
    if (init.method === "POST" && path === "write") return new Response(JSON.stringify({ write: `cs-${++writes}` }), { status: 201 });
    if (path === "write/cs-1") {
      // A read of cs-1 hangs from the second one on, and answers after Write another.
      if (aHeld) {
        await a.p;
        return new Response(JSON.stringify({ state: "sent", draft: "d-1" }), { status: 200 });
      }
      aHeld = true;
      return new Response(JSON.stringify({ state: "waiting", draft: "d-1", sending: false, leaves_at: null }), { status: 200 });
    }
    if (path === "write/cs-2") {
      await b.p;
      return new Response(JSON.stringify({ state: "waiting", draft: "d-2", sending: false, leaves_at: null }), { status: 200 });
    }
    return new Response("{}", { status: 404 });
  };
  const app = await mount("/src/views/Write.svelte", { start: { accounts: ACCOUNTS }, every: 10 });
  try {
    await fill(app);
    press(app);
    await until(() => app.target.querySelector(".done h1")?.textContent.includes("On its way to sam"));
    await new Promise((resolve) => setTimeout(resolve, 30));
    [...app.target.querySelectorAll("button")].find((x) => x.textContent.includes("Write another")).click();
    app.flushSync();
    await fill(app, "ana@example.com ");
    press(app);
    await until(() => app.target.querySelector(".done h1")?.textContent.includes("Saving"));
    // The read of cs-1 answers before the read of cs-2.
    a.open();
    await new Promise((resolve) => setTimeout(resolve, 50));
    app.flushSync();
    assert.match(text(app.target.querySelector(".done h1")), /Saving your mail/, "no state of cs-1 under cs-2");
    b.open();
    await until(() => app.target.querySelector(".done h1")?.textContent.includes("On its way to ana"));
  } finally {
    app.destroy();
    net.restore();
  }
});
