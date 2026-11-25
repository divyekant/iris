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
  topic: "Q4 numbers",
  conversation: [
    { id: "m-7", from: { name: null, address: "dk@example.com" }, date: "2026-11-16T10:00:00Z", to: "priya@example.com", you: true, hostile: false, text: "Here is the draft deck.", cut: false },
    { id: "m-8", from: { name: "Mallory", address: "mallory@bad.example" }, date: "2026-11-16T11:00:00Z", to: "dk@example.com", you: false, hostile: true, text: "Open this link now.", cut: true },
    { id: "m-9", from: { name: "Priya Shah", address: "priya@example.com" }, date: "2026-11-17T09:12:00Z", to: "dk@example.com", you: false, hostile: false, text: "Could you send the final Q4 numbers by Thursday?", cut: false },
  ],
  earlier: 2,
};

/** A fake engine. `answer(path, method, body)` gives [status, body], or throws for no answer. */
function serve(answer) {
  const asked = [];
  const saved = globalThis.fetch;
  globalThis.fetch = async (url, init = {}) => {
    const path = String(url).replace(/^\/api\//, "");
    const method = init.method ?? "GET";
    const body = typeof init.body?.arrayBuffer === "function" ? init.body : init.body ? JSON.parse(init.body) : undefined;
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

test("generated hashes: #/write/<id> opens a reply only for one canonical thread ID and no other part", () => {
  let seed = 23;
  const rand = (n) => ((seed = (seed * 1103515245 + 12345) % 2147483648), seed % n);
  const pieces = ["#/write", "/", "t-", "m-", "0", "1", "9", "x", "%", "%2F", "-", "write", "#"];
  let replies = 0;
  for (let i = 0; i < 5000; i++) {
    const start = ["#/write/t-", "#/write/", "#write/t-", ""][rand(4)];
    const hash = start + Array.from({ length: rand(6) }, () => pieces[rand(pieces.length)]).join("");
    const r = route.parse(hash);
    if (r.view !== "write") continue;
    const rest = hash.replace(/^#\/?/, "");
    if (r.thread === null) assert.match(rest, /^write\/?$/, hash);
    else {
      assert.equal(rest, `write/${r.thread}`, hash);
      assert.match(r.thread, route.THREAD_ID, hash);
      replies++;
    }
  }
  assert.ok(replies > 0, "some generated hashes open a reply");
  for (let n = 1; n < 2000; n += 7) assert.deepEqual(route.parse(route.href.reply(`t-${n}`)), { view: "write", thread: `t-${n}` });
});

test("the routes of the write view", () => {
  assert.deepEqual(route.parse("#/write"), { view: "write", thread: null });
  assert.deepEqual(route.parse("#/write/"), { view: "write", thread: null });
  assert.deepEqual(route.parse("#/write/t-4"), { view: "write", thread: "t-4" });
  assert.deepEqual(route.parse("#/write/m-4"), { view: "home" });
  assert.deepEqual(route.parse("#/write/t-4/x"), { view: "home" });
  assert.equal(route.href.reply("t-4"), "#/write/t-4");
  assert.ok(route.NAV.some(([link]) => link === "#/write"));
});

test("a typed address becomes a chip, and Send waits for a valid one and for text", async () => {
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
    assert.equal(send().disabled, true, "no text: Send waits (issue 741)");
    type(app.target.querySelector("textarea"), " \n\t");
    app.flushSync();
    assert.equal(send().disabled, true, "only white space: Send waits");
    type(app.target.querySelector("textarea"), "Hello");
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

test("a send with Bcc says that it waits for the Bcc check, with no time line", async () => {
  const net = serve((path, method) => {
    if (method === "POST" && path === "write") return [201, { write: "cs-3" }];
    if (path === "write/cs-3") return [200, { state: "waiting", draft: "d-7", sending: true, bcc_check: "gmail", leaves_at: new Date(Date.now() - 5_000).toISOString() }];
    return [404, { error: "Not found." }];
  });
  const app = await mount("/src/views/Write.svelte", { start: { accounts: ACCOUNTS }, every: 20 });
  try {
    await fill(app);
    press(app);
    await until(() => app.target.querySelector(".state h2")?.textContent.includes("On its way"));
    app.flushSync();
    assert.match(text(app.target), /Waits for the Bcc check of Gmail\./);
    assert.doesNotMatch(text(app.target), /Leaves now/);
    assert.equal(app.target.querySelector(".state .bar"), null);
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
    await until(() => app.target.querySelector(".state h2")?.textContent.includes("On its way"));
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

test("a reply opens under the conversation, names its recipients and names the parent", async () => {
  const net = serve((path, method) => (method === "POST" ? [201, { write: "cs-4" }] : [200, { state: "stopped", reason: "You stopped this mail.", draft: null }]));
  const app = await mount("/src/views/Write.svelte", { start: { thread: "t-2", reply: REPLY }, every: 20 });
  try {
    assert.equal(text(app.target.querySelector("h1")), "Q4 numbers");
    assert.match(text(app.target), /2 earlier mails are not shown\./);
    const mails = [...app.target.querySelectorAll(".msg")];
    assert.equal(mails.length, 3, "each mail of the conversation, oldest first");
    assert.match(text(mails[0]), /You.*Here is the draft deck\./);
    assert.match(text(mails[1]), /This mail looks hostile/);
    assert.equal(mails[1].querySelector("a.more").getAttribute("href"), "#/mail/m-8", "a cut text links to the whole mail");
    assert.match(text(mails[2]), /Priya Shah.*Could you send the final Q4 numbers by Thursday\?/);
    assert.ok(mails[0].classList.contains("old") && !mails[2].classList.contains("old"), "the parent reads in full size");
    // The form is the last entry: after the parent, and it names the recipients.
    assert.ok(mails[2].compareDocumentPosition(app.target.querySelector("form")) & window.Node.DOCUMENT_POSITION_FOLLOWING);
    assert.match(text(app.target.querySelector(".who")), /Reply to Priya Shah/);
    assert.equal(app.target.querySelector("#write-to"), null, "the chips stay closed until Change");
    assert.equal(app.target.querySelector("input.subject"), null, "Iris sets the subject of a reply");
    type(app.target.querySelector("textarea"), "Yes, on Wednesday.\n");
    app.flushSync();
    press(app);
    await until(() => app.target.querySelector(".state h2")?.textContent.includes("did not leave"));
    const mine = [...app.target.querySelectorAll(".msg")].at(-1);
    assert.match(text(mine), /You To priya@example\.com Yes, on Wednesday\./, "the mail shows as the last entry");
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

test("Change opens the recipients of a reply as chips", async () => {
  const net = serve(() => [201, { write: "cs-5" }]);
  const app = await mount("/src/views/Write.svelte", { start: { thread: "t-2", reply: REPLY }, every: 20 });
  try {
    [...app.target.querySelectorAll("button")].find((b) => b.textContent === "Change").click();
    app.flushSync();
    assert.ok(app.target.querySelector("#write-to"));
    type(app.target.querySelector("#write-to"), "sam@okafor.example ");
    app.flushSync();
    const chips = [...app.target.querySelectorAll(".chip")].map((c) => c.firstChild.textContent.trim());
    assert.deepEqual(chips, ["priya@example.com", "sam@okafor.example"]);
  } finally {
    app.destroy();
    net.restore();
  }
});

test("a reply with no valid recipient opens the chips at once", async () => {
  const net = serve(() => [201, { write: "cs-6" }]);
  const app = await mount("/src/views/Write.svelte", { start: { thread: "t-2", reply: { ...REPLY, to: [] } }, every: 20 });
  try {
    assert.ok(app.target.querySelector("#write-to"));
    assert.equal(app.target.querySelector(".who"), null);
  } finally {
    app.destroy();
    net.restore();
  }
});

test("a reload shows an unfinished copy address, so Send never sends to an address that the form hides", async () => {
  const net = serve(() => [201, { write: "cs-7" }]);
  let app = await mount("/src/views/Write.svelte", { start: { accounts: ACCOUNTS }, every: 20 });
  try {
    await fill(app);
    [...app.target.querySelectorAll("button")].find((b) => b.textContent === "Add copy").click();
    app.flushSync();
    type(app.target.querySelector("#write-cc"), "ana@example.com");
    app.flushSync();
    app.destroy();
    app = await mount("/src/views/Write.svelte", { start: { accounts: ACCOUNTS }, every: 20 });
    assert.equal(app.target.querySelector("#write-cc")?.value, "ana@example.com", "the copy field shows after the reload");
  } finally {
    app.destroy();
    net.restore();
  }
});

test("a reply with restored open address text shows the fields, not the one line", async () => {
  const saved = { account: "a-1", to: ["priya@example.com"], cc: [], subject: "", body: "Yes.", key: "k-0123456789abcdef", sig: "", flags: [], toText: "", ccText: "ana@example.com" };
  window.sessionStorage.setItem("iris.write.t-2", JSON.stringify(saved));
  const net = serve(() => [201, { write: "cs-9" }]);
  const app = await mount("/src/views/Write.svelte", { start: { thread: "t-2", reply: REPLY }, every: 20 });
  try {
    assert.equal(app.target.querySelector(".who"), null);
    assert.equal(app.target.querySelector("#write-cc")?.value, "ana@example.com");
  } finally {
    app.destroy();
    net.restore();
  }
});

test("a press with an unfinished address and a lost answer: the retry sends the same key", async () => {
  let lost = true;
  const net = serve((path, method) => {
    if (method === "POST" && lost) throw new Error("no answer");
    if (method === "POST") return [200, { write: "cs-8" }];
    return [200, { state: "saving" }];
  });
  const app = await mount("/src/views/Write.svelte", { start: { accounts: ACCOUNTS }, every: 20 });
  try {
    await fill(app, "sam@okafor.example");
    press(app);
    await until(() => text(app.target).includes("Iris does not know if it took the mail"));
    lost = false;
    press(app);
    await until(() => net.posts().length === 2);
    const [first, second] = net.posts();
    assert.deepEqual(first.body.to, ["sam@okafor.example"]);
    assert.equal(second.body.key, first.body.key, "one content, one key");
    assert.equal(JSON.parse(window.sessionStorage.getItem("iris.write.new") ?? "null")?.key ?? first.body.key, first.body.key);
  } finally {
    app.destroy();
    net.restore();
  }
});

test("Write it for me sends the intent to the engine and puts its draft in the text", async () => {
  let release;
  const held = new Promise((r) => (release = r));
  const net = serve((path, method) => {
    if (method === "POST" && path === "threads/t-2/draft") return [200, { body: "Hi Priya,\n\nYes, on Wednesday.", based_on: "m-9", samples: 3, unknown_cost: false, overrun: false }];
    return [404, { error: "Not found." }];
  });
  const fetch = globalThis.fetch;
  globalThis.fetch = async (...a) => (a[0].toString().endsWith("/draft") ? (await held, fetch(...a)) : fetch(...a));
  const app = await mount("/src/views/Write.svelte", { start: { thread: "t-2", reply: REPLY }, every: 20 });
  try {
    type(app.target.querySelector(".intent input"), "yes, Wednesday");
    app.flushSync();
    [...app.target.querySelectorAll("button")].find((b) => b.textContent === "Write it for me").click();
    app.flushSync();
    assert.equal(app.target.querySelector("fieldset").disabled, true, "the form does not change while Iris writes");
    release();
    await until(() => app.target.querySelector("textarea").value.startsWith("Hi Priya"));
    const [post] = net.posts();
    assert.equal(post.path, "threads/t-2/draft");
    assert.deepEqual(post.body, { intent: "yes, Wednesday", parent: "m-9", to: ["priya@example.com"] }, "the draft answers the mail that the form shows, to the recipients of the form");
    assert.equal(app.target.querySelector("fieldset").disabled, false);
    assert.equal(app.target.querySelector("button[type=submit]").disabled, false, "the person can send the draft");
  } finally {
    globalThis.fetch = fetch;
    app.destroy();
    net.restore();
  }
});

test("a refused draft shows its reason and keeps the text", async () => {
  const net = serve(() => [409, { error: "This thread has hostile mail. Iris writes no draft for it." }]);
  const app = await mount("/src/views/Write.svelte", { start: { thread: "t-2", reply: REPLY }, every: 20 });
  try {
    type(app.target.querySelector("textarea"), "My own words.");
    app.flushSync();
    [...app.target.querySelectorAll("button")].find((b) => b.textContent === "Write it for me").click();
    await until(() => app.target.querySelector(".note"));
    assert.match(text(app.target.querySelector(".note")), /hostile mail/);
    assert.equal(app.target.querySelector("textarea").value, "My own words.");
  } finally {
    app.destroy();
    net.restore();
  }
});

test("a draft for another mail than the one the form shows does not go in the text", async () => {
  const net = serve(() => [200, { body: "Text for a newer mail.", based_on: "m-10", samples: 1, unknown_cost: false, overrun: false }]);
  const app = await mount("/src/views/Write.svelte", { start: { thread: "t-2", reply: REPLY }, every: 20 });
  try {
    type(app.target.querySelector("textarea"), "My own words.");
    app.flushSync();
    [...app.target.querySelectorAll("button")].find((b) => b.textContent === "Write it for me").click();
    await until(() => app.target.querySelector(".note"));
    assert.match(text(app.target.querySelector(".note")), /A new mail came into this thread/);
    assert.equal(app.target.querySelector("textarea").value, "My own words.");
  } finally {
    app.destroy();
    net.restore();
  }
});

test("the intent stays when the person leaves the form and comes back", async () => {
  const net = serve(() => [404, {}]);
  let app = await mount("/src/views/Write.svelte", { start: { thread: "t-2", reply: REPLY }, every: 20 });
  try {
    type(app.target.querySelector(".intent input"), "yes, Wednesday");
    app.flushSync();
    app.destroy();
    app = await mount("/src/views/Write.svelte", { start: { thread: "t-2", reply: REPLY }, every: 20 });
    assert.equal(app.target.querySelector(".intent input").value, "yes, Wednesday");
  } finally {
    app.destroy();
    net.restore();
  }
});

test("a new mail has no intent line: Iris drafts replies only", async () => {
  const net = serve(() => [404, {}]);
  const app = await mount("/src/views/Write.svelte", { start: { accounts: ACCOUNTS }, every: 20 });
  try {
    assert.equal(app.target.querySelector(".intent"), null);
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
    await until(() => app.target.querySelector(".state h2")?.textContent.includes("On its way to sam"));
    await new Promise((resolve) => setTimeout(resolve, 30));
    [...app.target.querySelectorAll("button")].find((x) => x.textContent.includes("Write another")).click();
    app.flushSync();
    await fill(app, "ana@example.com ");
    press(app);
    await until(() => app.target.querySelector(".state h2")?.textContent.includes("Saving"));
    // The read of cs-1 answers before the read of cs-2.
    a.open();
    await new Promise((resolve) => setTimeout(resolve, 50));
    app.flushSync();
    assert.match(text(app.target.querySelector(".state h2")), /Saving your mail/, "no state of cs-1 under cs-2");
    b.open();
    await until(() => app.target.querySelector(".state h2")?.textContent.includes("On its way to ana"));
  } finally {
    app.destroy();
    net.restore();
  }
});

test("the form does not change while the press runs, and the state names the mail that the engine took", async () => {
  let answer;
  const late = new Promise((resolve) => (answer = resolve));
  const net = serve(() => [500, {}]);
  globalThis.fetch = async (url, init = {}) => {
    const path = String(url).replace(/^\/api\//, "");
    if (init.method === "POST" && path === "write") {
      await late;
      return new Response(JSON.stringify({ write: "cs-3" }), { status: 201 });
    }
    return new Response(JSON.stringify({ state: "waiting", draft: "d-3", sending: false, leaves_at: null }), { status: 200 });
  };
  const app = await mount("/src/views/Write.svelte", { start: { accounts: ACCOUNTS }, every: 20 });
  try {
    await fill(app);
    press(app);
    app.flushSync();
    assert.equal(app.target.querySelector("fieldset").disabled, true, "the form is frozen");
    // An edit that gets through (a script, an old browser) does not change what the state names.
    app.target.querySelector(".chip button").click();
    type(app.target.querySelector("#write-to"), "ana@example.com ");
    app.flushSync();
    answer();
    await until(() => app.target.querySelector(".state h2"));
    await until(() => app.target.querySelector(".state h2").textContent.includes("On its way"));
    assert.match(text(app.target.querySelector(".state h2")), /On its way to sam@okafor\.example/);
  } finally {
    app.destroy();
    net.restore();
  }
});

test("a reload while the write is saving keeps its state and Stop this mail", async () => {
  let stopped = false;
  const net = serve((path, method) => {
    if (method === "POST" && path === "write") return [201, { write: "cs-3" }];
    if (method === "POST" && path === "changesets/cs-3/withdraw") return (stopped = true), [200, {}];
    if (path === "write/cs-3") return [200, stopped ? { state: "stopped", reason: "You stopped this mail.", draft: null } : { state: "saving" }];
    return [404, { error: "Not found." }];
  });
  let app = await mount("/src/views/Write.svelte", { start: { accounts: ACCOUNTS }, every: 20 });
  try {
    await fill(app);
    press(app);
    await until(() => app.target.querySelector(".state h2")?.textContent.includes("Saving your mail"));
    // The reload: a new view reads the accepted write from the storage, not a fresh form.
    app.destroy();
    app = await mount("/src/views/Write.svelte", { start: { accounts: ACCOUNTS }, every: 20 });
    await until(() => app.target.querySelector(".state h2")?.textContent.includes("Saving your mail"));
    app.flushSync();
    assert.equal(app.target.querySelector("form"), null, "no fresh form");
    assert.match(text(app.target), /To sam@okafor\.example/);
    assert.match(text(app.target), /Are you free\?/);
    const stop = [...app.target.querySelectorAll("button")].find((b) => b.textContent === "Stop this mail");
    stop.click();
    await until(() => app.target.querySelector(".state h2")?.textContent.includes("did not leave"));
    assert.ok(net.posts().some((p) => p.path === "changesets/cs-3/withdraw"));
    // A terminal state ends the record: the next view opens a fresh form.
    app.destroy();
    app = await mount("/src/views/Write.svelte", { start: { accounts: ACCOUNTS }, every: 20 });
    assert.ok(app.target.querySelector("form"), "a fresh form after the end of the write");
  } finally {
    app.destroy();
    net.restore();
  }
});

test("Write another ends the record of the last write", async () => {
  const net = serve((path, method) => {
    if (method === "POST" && path === "write") return [201, { write: "cs-3" }];
    if (path === "write/cs-3") return [200, { state: "waiting", draft: "d-7", sending: false, leaves_at: null }];
    return [404, { error: "Not found." }];
  });
  let app = await mount("/src/views/Write.svelte", { start: { accounts: ACCOUNTS }, every: 20 });
  try {
    await fill(app);
    press(app);
    await until(() => [...app.target.querySelectorAll("button")].some((b) => b.textContent === "Write another"));
    [...app.target.querySelectorAll("button")].find((b) => b.textContent === "Write another").click();
    app.destroy();
    app = await mount("/src/views/Write.svelte", { start: { accounts: ACCOUNTS }, every: 20 });
    assert.ok(app.target.querySelector("form"), "a fresh form");
  } finally {
    app.destroy();
    net.restore();
  }
});

test("a late answer of a left view does not replace the record of a newer write", async () => {
  let first;
  const late = new Promise((r) => (first = r));
  const saved = globalThis.fetch;
  let posts = 0;
  globalThis.fetch = async (url, init = {}) => {
    const path = String(url).replace(/^\/api\//, "");
    if (init.method === "POST" && path === "write") {
      posts += 1;
      if (posts === 1) await late;
      return new Response(JSON.stringify({ write: posts === 1 ? "cs-3" : "cs-4" }), { status: 201 });
    }
    return new Response(JSON.stringify({ state: "saving" }), { status: 200 });
  };
  let app = await mount("/src/views/Write.svelte", { start: { accounts: ACCOUNTS }, every: 20 });
  try {
    await fill(app);
    press(app);
    await until(() => posts === 1);
    // The person leaves while the first press waits, comes back and sends another mail.
    app.destroy();
    app = await mount("/src/views/Write.svelte", { start: { accounts: ACCOUNTS }, every: 20 });
    await fill(app, "ana@example.com ");
    press(app);
    await until(() => app.target.querySelector(".state h2")?.textContent.includes("Saving your mail"));
    // The first answer arrives last.
    first();
    await new Promise((r) => setTimeout(r, 30));
    app.destroy();
    app = await mount("/src/views/Write.svelte", { start: { accounts: ACCOUNTS }, every: 20 });
    await until(() => app.target.querySelector(".state"));
    app.flushSync();
    assert.equal(JSON.parse(window.sessionStorage.getItem("iris.write.new.taken")).write, "cs-4", "the record names the newer write");
    assert.match(text(app.target), /To sam@okafor\.example, ana@example\.com/, "the reload shows the newer mail");
  } finally {
    app.destroy();
    globalThis.fetch = saved;
  }
});

test("a draft that Iris saved for the thread fills a new reply form, and no other form (issue #743)", async () => {
  const saved = { thread: "t-2", based_on: "m-9", nudge: false, to: null, item: null, body: "Hi Priya,\n\nYes, Friday at 10." };
  for (const [name, draft, form, want] of [
    ["a draft of the shown mail", saved, null, saved.body],
    ["a draft of an older mail", { ...saved, based_on: "m-8" }, null, ""],
    ["a nudge", { ...saved, nudge: true, to: "sam@example.com", item: "c-4" }, null, ""],
    ["a form that the person started", saved, "My own words.", "My own words."],
  ]) {
    window.sessionStorage.clear();
    const net = serve((path, method) => (method === "GET" && path === "threads/t-2/draft" ? [200, draft] : [404, { error: "Not found." }]));
    if (form) {
      const first = await mount("/src/views/Write.svelte", { start: { thread: "t-2", reply: REPLY }, every: 20 });
      type(first.target.querySelector("textarea"), form);
      first.flushSync();
      first.destroy();
    }
    const app = await mount("/src/views/Write.svelte", { start: { thread: "t-2", reply: REPLY }, every: 20 });
    try {
      await until(() => net.asked.some((a) => a.path === "threads/t-2/draft") || form);
      await new Promise((r) => setTimeout(r, 30));
      app.flushSync();
      assert.equal(app.target.querySelector("textarea").value, want, name);
      assert.equal(!!app.target.querySelector(".saved"), want === saved.body, name);
    } finally {
      app.destroy();
      net.restore();
    }
  }
});

test("an untouched saved form still gets the saved draft, and a late read does not change the form after Write it for me (issue 743)", async () => {
  const saved = { thread: "t-2", based_on: "m-9", nudge: false, to: null, item: null, body: "Hi Priya,\n\nYes, Friday at 10." };
  // The person opened the reply, changed nothing and left: the form is saved with no text.
  let net = serve(() => [404, { error: "Not found." }]);
  let first = await mount("/src/views/Write.svelte", { start: { thread: "t-2", reply: REPLY }, every: 20 });
  first.flushSync();
  first.destroy();
  net.restore();
  assert.ok(window.sessionStorage.getItem("iris.write.t-2"), "the untouched form is saved");
  net = serve((path, method) => (method === "GET" && path === "threads/t-2/draft" ? [200, saved] : [404, { error: "Not found." }]));
  let app = await mount("/src/views/Write.svelte", { start: { thread: "t-2", reply: REPLY }, every: 20 });
  try {
    await until(() => app.target.querySelector("textarea").value === saved.body);
  } finally {
    app.destroy();
    net.restore();
  }

  // The read of the saved draft answers while Write it for me runs: the form does not change, and keeps the new draft.
  window.sessionStorage.clear();
  let releaseGet, releasePost;
  const heldGet = new Promise((r) => (releaseGet = r));
  const heldPost = new Promise((r) => (releasePost = r));
  net = serve((path, method) => {
    if (method === "GET" && path === "threads/t-2/draft") return [200, saved];
    if (method === "POST" && path === "threads/t-2/draft") return [200, { body: "Hi Priya,\n\nThe new one.", based_on: "m-9", samples: 3, unknown_cost: false, overrun: false }];
    return [404, { error: "Not found." }];
  });
  const fetch = globalThis.fetch;
  globalThis.fetch = async (...a) => {
    if (!String(a[0]).endsWith("/draft")) return fetch(...a);
    await ((a[1]?.method ?? "GET") === "GET" ? heldGet : heldPost);
    return fetch(...a);
  };
  app = await mount("/src/views/Write.svelte", { start: { thread: "t-2", reply: REPLY }, every: 20 });
  try {
    [...app.target.querySelectorAll("button")].find((b) => b.textContent === "Write it for me").click();
    app.flushSync();
    releaseGet();
    await new Promise((r) => setTimeout(r, 30));
    app.flushSync();
    assert.equal(app.target.querySelector("textarea").value, "", "the form does not change while Iris writes");
    releasePost();
    await until(() => app.target.querySelector("textarea").value.includes("The new one."));
    assert.equal(app.target.querySelector(".saved"), null);
  } finally {
    globalThis.fetch = fetch;
    app.destroy();
    net.restore();
  }
});

test("a saved form with a changed intent or recipient does not get the saved draft (issue 743)", async () => {
  const saved = { thread: "t-2", based_on: "m-9", nudge: false, to: null, item: null, body: "Hi Priya,\n\nYes, Friday at 10." };
  for (const [name, change] of [
    ["an intent", (app) => type(app.target.querySelector(".intent input"), "no, Monday")],
    ["a recipient", (app) => {
      [...app.target.querySelectorAll("button")].find((b) => b.textContent === "Change").click();
      app.flushSync();
      type(app.target.querySelector("#write-to"), "sam@example.com ");
    }],
  ]) {
    window.sessionStorage.clear();
    let net = serve(() => [404, { error: "Not found." }]);
    const first = await mount("/src/views/Write.svelte", { start: { thread: "t-2", reply: REPLY }, every: 20 });
    // The first view reads the draft too: let that read end before the next engine.
    await new Promise((r) => setTimeout(r, 20));
    change(first);
    first.flushSync();
    first.destroy();
    net.restore();
    net = serve((path, method) => (method === "GET" && path === "threads/t-2/draft" ? [200, saved] : [404, { error: "Not found." }]));
    const app = await mount("/src/views/Write.svelte", { start: { thread: "t-2", reply: REPLY }, every: 20 });
    try {
      await new Promise((r) => setTimeout(r, 40));
      app.flushSync();
      assert.equal(app.target.querySelector("textarea").value, "", name);
      assert.equal(net.asked.filter((a) => a.path === "threads/t-2/draft").length, 0, `${name}: no read`);
    } finally {
      app.destroy();
      net.restore();
    }
  }
});

test("a saved form whose recipients changed order does not get the saved draft (issue 831)", async () => {
  // The voice draft greets the first recipient: a form with the same people in another order is a form that the person changed.
  const two = { ...REPLY, to: ["priya@example.com", "sam@example.com"] };
  const saved = { thread: "t-2", based_on: "m-9", nudge: false, to: null, item: null, body: "Hi Priya,\n\nYes, Friday at 10." };
  for (const [name, order, want] of [
    ["the same order", (to) => to, saved.body],
    ["another order", (to) => [...to].reverse(), ""],
  ]) {
    window.sessionStorage.clear();
    let net = serve(() => [404, { error: "Not found." }]);
    const first = await mount("/src/views/Write.svelte", { start: { thread: "t-2", reply: two }, every: 20 });
    await new Promise((r) => setTimeout(r, 20));
    first.flushSync();
    first.destroy();
    net.restore();
    const form = JSON.parse(window.sessionStorage.getItem("iris.write.t-2"));
    window.sessionStorage.setItem("iris.write.t-2", JSON.stringify({ ...form, to: order(form.to) }));
    net = serve((path, method) => (method === "GET" && path === "threads/t-2/draft" ? [200, saved] : [404, { error: "Not found." }]));
    const app = await mount("/src/views/Write.svelte", { start: { thread: "t-2", reply: two }, every: 20 });
    try {
      await new Promise((r) => setTimeout(r, 40));
      app.flushSync();
      assert.equal(app.target.querySelector("textarea").value, want, name);
    } finally {
      app.destroy();
      net.restore();
    }
  }
});

test("a change to the intent or a recipient while the saved draft loads keeps the draft out (issue 743)", async () => {
  const saved = { thread: "t-2", based_on: "m-9", nudge: false, to: null, item: null, body: "Hi Priya,\n\nYes, Friday at 10." };
  for (const [name, change] of [
    ["an intent", (app) => type(app.target.querySelector(".intent input"), "no, Monday")],
    ["a recipient", (app) => {
      [...app.target.querySelectorAll("button")].find((b) => b.textContent === "Change").click();
      app.flushSync();
      type(app.target.querySelector("#write-to"), "sam@example.com ");
    }],
    ["open address text", (app) => {
      [...app.target.querySelectorAll("button")].find((b) => b.textContent === "Change").click();
      app.flushSync();
      type(app.target.querySelector("#write-to"), "sam@exa");
    }],
  ]) {
    window.sessionStorage.clear();
    let release;
    const held = new Promise((r) => (release = r));
    const net = serve((path, method) => (method === "GET" && path === "threads/t-2/draft" ? [200, saved] : [404, { error: "Not found." }]));
    const fetch = globalThis.fetch;
    globalThis.fetch = async (...a) => {
      if (String(a[0]).endsWith("/draft")) await held;
      return fetch(...a);
    };
    const app = await mount("/src/views/Write.svelte", { start: { thread: "t-2", reply: REPLY }, every: 20 });
    try {
      change(app);
      app.flushSync();
      release();
      await new Promise((r) => setTimeout(r, 30));
      app.flushSync();
      assert.equal(app.target.querySelector("textarea").value, "", name);
      assert.equal(app.target.querySelector(".saved"), null, name);
    } finally {
      globalThis.fetch = fetch;
      app.destroy();
      net.restore();
    }
  }
});

test("Add Bcc opens the blind copy, a chosen file goes to the engine first, and Send sends both (send content spec)", async () => {
  const net = serve((path, method) => {
    if (path === "attachments") return [201, { sha256: "ab".repeat(32), size: 5 }];
    if (path === "write" && method === "POST") return [201, { write: "cs-3" }];
    return [200, { state: "saving" }];
  });
  const app = await mount("/src/views/Write.svelte", { start: { accounts: ACCOUNTS }, every: 60_000 });
  try {
    await fill(app);
    assert.equal(app.target.querySelector("#write-bcc"), null, "the blind copy waits for its button");
    [...app.target.querySelectorAll("button")].find((b) => b.textContent === "Add Bcc").click();
    app.flushSync();
    type(app.target.querySelector("#write-bcc"), "lee@example.org ");
    app.flushSync();
    const input = app.target.querySelector("input[type=file]");
    const file = new window.File(["hello"], "notes.txt", { type: "text/plain;charset=utf-8" });
    Object.defineProperty(input, "files", { configurable: true, value: [file] });
    input.dispatchEvent(new window.Event("change", { bubbles: true }));
    await until(() => app.target.querySelector(".files .chip"));
    app.flushSync();
    assert.match(text(app.target.querySelector(".files")), /notes\.txt\s*5 B/);
    const up = net.posts().find((a) => a.path === "attachments");
    assert.equal(typeof up.body.arrayBuffer, "function", "the file goes as its bytes");
    press(app);
    await until(() => net.posts().some((a) => a.path === "write"));
    const mail = net.posts().find((a) => a.path === "write").body;
    assert.deepEqual(mail.bcc, ["lee@example.org"]);
    assert.deepEqual(mail.attachments, [{ name: "notes.txt", type: "text/plain", size: 5, sha256: "ab".repeat(32) }]);
  } finally {
    app.destroy();
    net.restore();
  }
});

test("a file that the engine refuses names its reason and does not join the mail", async () => {
  const net = serve((path) => (path === "attachments" ? [413, { error: "The file is too large." }] : [200, {}]));
  const app = await mount("/src/views/Write.svelte", { start: { accounts: ACCOUNTS }, every: 60_000 });
  try {
    await fill(app);
    const input = app.target.querySelector("input[type=file]");
    Object.defineProperty(input, "files", { configurable: true, value: [new window.File(["x"], "big.bin")] });
    input.dispatchEvent(new window.Event("change", { bubbles: true }));
    await until(() => app.target.querySelector(".note"));
    app.flushSync();
    assert.match(text(app.target.querySelector(".note")), /big\.bin: The file is too large\./);
    assert.equal(app.target.querySelector(".files .chip"), null);
    assert.equal(app.target.querySelector("button[type=submit]").disabled, false);
  } finally {
    app.destroy();
    net.restore();
  }
});

test("Change and send opens the form with the draft; update sends the draft, new does not (draft change spec, section 5)", async () => {
  for (const change of ["update", "new", null]) {
    window.sessionStorage.clear();
    let n = 0;
    const net = serve((path, method) => {
      if (method === "POST" && path === "write") return [201, { write: `cs-${++n}` }];
      if (path === "write/cs-1") return [200, { state: "stopped", reason: "You stopped this mail.", draft: "d-1", change }];
      if (path === "write/cs-2") return [200, { state: "saving" }];
      if (path === "drafts/d-1")
        return [200, { draft: "d-1", account: "a-1", thread: null, to: ["sam@okafor.example"], cc: ["kim@example.com"], bcc: [],
                       subject: "Dinner on Saturday?", body: "Hi Sam,\n\nAre you free?\n", attachments: [] }];
      return [404, { error: "Not found." }];
    });
    const app = await mount("/src/views/Write.svelte", { start: { accounts: ACCOUNTS }, every: 20 });
    try {
      await fill(app);
      press(app);
      await until(() => text(app.target).includes("This mail did not leave"));
      const button = () => [...app.target.querySelectorAll("button")].find((b) => b.textContent === "Change and send");
      if (change === null) {
        assert.equal(button(), undefined, "no change: no button");
        continue;
      }
      button().click();
      await until(() => app.target.querySelector("form"));
      app.flushSync();
      assert.equal(app.target.querySelector("input.subject").value, "Dinner on Saturday?");
      assert.equal(app.target.querySelector("textarea").value, "Hi Sam,\n\nAre you free?\n");
      type(app.target.querySelector("textarea"), "Hi Sam,\n\nAre you free on Sunday?\n");
      app.flushSync();
      press(app);
      await until(() => net.posts().filter((p) => p.path === "write").length === 2);
      const [first, second] = net.posts().filter((p) => p.path === "write").map((p) => p.body);
      assert.deepEqual(second.to, ["sam@okafor.example"]);
      assert.deepEqual(second.cc, ["kim@example.com"]);
      assert.equal(second.body, "Hi Sam,\n\nAre you free on Sunday?\n");
      assert.equal(second.draft, change === "update" ? "d-1" : undefined);
      assert.notEqual(second.key, first.key, "a new content gets a new key");
    } finally {
      app.destroy();
      net.restore();
    }
  }
});
