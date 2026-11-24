// The stranger gate in a DOM (spec 1 section 6.7): Accept, Ignore, Ask again,
// Undo, the refusals of the engine, and the next page.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

const fx = await load("/dev/fixtures.ts");
const text = (el) => el.textContent.replace(/\s+/g, " ");
const button = (view, name) => [...view.target.querySelectorAll("button")].find((b) => text(b).trim() === name);
const buttons = (view, name) => [...view.target.querySelectorAll("button")].filter((b) => text(b).trim() === name);

// Answer each request with `answer(path)`: [status, body].
function serve(answer) {
  const asked = [];
  globalThis.fetch = async (url, init) => {
    const path = String(url).replace(/^\/api\//, "");
    // A brief line owed to you reads its ready draft (card R3-07): not a request of the gate.
    if (path.endsWith("/draft")) return new Response(JSON.stringify({ error: "Not found." }), { status: 404 });
    asked.push([init?.method ?? "GET", path]);
    const [status, out] = await answer(path);
    return new Response(JSON.stringify(out), { status, headers: { "content-type": "application/json" } });
  };
  return asked;
}

const row = (id) => ({ id, address: "a@b.example", name: null, kind: "person", decided_by: "person", declined: false });
// The sender row that the engine gives for the answer of `path` (senders/<id>/<answer>).
const rowFor = (path) => {
  const [, id, verb] = path.split("/");
  return verb === "accept" ? row(id) : { ...row(id), kind: "stranger", decided_by: "engine", declined: verb === "decline" };
};

test("Accept answers the sender, and Undo asks again", async () => {
  const asked = serve((path) => [200, rowFor(path)]);
  let answers = 0;
  const view = await mount("/src/views/Gate.svelte", { page: fx.gate, declined: 1, onanswer: () => answers++ });
  try {
    assert.equal(buttons(view, "Accept").length, 3);
    buttons(view, "Accept")[0].click();
    await until(() => text(view.target).includes("You know Maria Ruiz now."));
    assert.deepEqual(asked, [["POST", "senders/p-71/accept"]]);
    assert.equal(buttons(view, "Accept").length, 2, "the other senders still wait");
    assert.equal(answers, 1);

    button(view, "Undo").click();
    await until(() => buttons(view, "Accept").length === 3);
    assert.deepEqual(asked[1], ["POST", "senders/p-71/reopen"]);
    assert.equal(answers, 2);
    assert.match(text(view.target), /Ignored 1/);
  } finally {
    view.destroy();
  }
});

test("a refusal of the engine shows its words, and the sender keeps its buttons", async () => {
  const words = "This sender is not at the gate now. Look at the brief again.";
  serve(() => [409, { error: words }]);
  const view = await mount("/src/views/Gate.svelte", { page: fx.gate });
  try {
    buttons(view, "Ignore")[1].click();
    await until(() => text(view.target).includes(words));
    assert.equal(buttons(view, "Ignore").length, 3);
    assert.doesNotMatch(text(view.target), /out of your brief/);
  } finally {
    view.destroy();
  }
});

test("on the list of ignored senders, Ask again reopens, and Undo ignores again", async () => {
  const asked = serve((path) => [200, rowFor(path)]);
  const view = await mount("/src/views/Gate.svelte", { page: fx.ignored, declinedList: true, title: "Senders that you ignored" });
  try {
    assert.equal(button(view, "Ignore"), undefined);
    button(view, "Ask again").click();
    await until(() => text(view.target).includes("Iris asks you about Dana Brooks again."));
    button(view, "Undo").click();
    await until(() => button(view, "Ask again"));
    assert.deepEqual(asked, [
      ["POST", "senders/p-74/reopen"],
      ["POST", "senders/p-74/decline"],
    ]);
  } finally {
    view.destroy();
  }
});

test("Show more senders loads the next page once", async () => {
  const [first, second] = [fx.gate.senders.slice(0, 2), fx.gate.senders.slice(2)];
  const asked = serve(() => [200, { count: 3, senders: second, next: null }]);
  const view = await mount("/src/views/Gate.svelte", { page: { count: 3, senders: first, next: "k1" }, more: "gate?limit=2" });
  try {
    button(view, "Show more senders").click();
    await until(() => text(view.target).includes("billing@vendorhub.example"));
    assert.deepEqual(asked, [["GET", "gate?limit=2&after=k1"]]);
    assert.equal(buttons(view, "Accept").length, 3);
    assert.equal(button(view, "Show more senders"), undefined);
  } finally {
    view.destroy();
  }
});

test("in the brief, an answered sender keeps its row and its Undo after the brief loads again", async () => {
  // After the accept, the engine gives a brief with Maria out of the gate and her ask under You owe.
  const after = { ...fx.brief, gate: { count: 2, senders: fx.gate.senders.slice(1), next: null } };
  const asked = serve((path) => (path === "brief" ? [200, after] : [200, row("p-71")]));
  const view = await mount("/src/views/Brief.svelte", { watches: [], proposed: null, questions: [], brief: fx.brief, now: new Date("2026-11-16T09:05:00Z"), poll: 0 });
  try {
    const gate = () => view.target.querySelector("#strangers").closest("section");
    [...gate().querySelectorAll("button")].find((b) => b.textContent.trim() === "Accept").click();
    await until(() => asked.some(([, p]) => p === "brief"));
    await until(() => text(gate()).includes("2"));
    const t = text(gate());
    assert.match(t, /You know Maria Ruiz now\. Undo/);
    // The row keeps its place: first, before the senders that still wait.
    assert.ok(t.indexOf("Maria Ruiz") < t.indexOf("Leo Park"), t);
    assert.equal([...gate().querySelectorAll("button")].filter((b) => b.textContent.trim() === "Accept").length, 2);
    assert.deepEqual(asked.slice(0, 2), [
      ["POST", "senders/p-71/accept"],
      ["GET", "brief"],
    ]);
  } finally {
    view.destroy();
  }
});

test("Undo stays after you answer the last sender of the gate", async () => {
  const only = { ...fx.brief, gate: { count: 1, senders: fx.gate.senders.slice(0, 1), next: null }, declined: 0 };
  const after = { ...fx.brief, gate: { count: 0, senders: [], next: null }, declined: 0 };
  const asked = serve((path) => (path === "brief" ? [200, after] : [200, row("p-71")]));
  const view = await mount("/src/views/Brief.svelte", { watches: [], proposed: null, questions: [], brief: only, now: new Date("2026-11-16T09:05:00Z"), poll: 0 });
  try {
    button(view, "Accept").click();
    await until(() => asked.some(([, p]) => p === "brief"));
    await new Promise((r) => setTimeout(r, 50));
    assert.match(text(view.target), /You know Maria Ruiz now\. Undo/);
    button(view, "Undo").click();
    await until(() => asked.some(([, p]) => p === "senders/p-71/reopen"));
  } finally {
    view.destroy();
  }
});

test("a sender takes one answer at a time, also while another sender waits", async () => {
  const held = [];
  const asked = [];
  globalThis.fetch = (url) => {
    const path = String(url).replace(/^\/api\//, "");
    asked.push(path);
    return new Promise((done) => held.push(() => done(new Response(JSON.stringify(rowFor(path)), { status: 200 }))));
  };
  const view = await mount("/src/views/Gate.svelte", { page: fx.gate });
  try {
    buttons(view, "Accept")[0].click();
    await until(() => asked.length === 1);
    buttons(view, "Ignore")[1].click();
    await until(() => asked.length === 2);
    view.flushSync();
    // Maria's answer still waits: her buttons stay off, and a click sends nothing.
    assert.equal(buttons(view, "Accept")[0].disabled, true);
    assert.equal(buttons(view, "Ignore")[0].disabled, true);
    buttons(view, "Ignore")[0].click();
    await new Promise((r) => setTimeout(r, 30));
    assert.deepEqual(asked, ["senders/p-71/accept", "senders/p-72/decline"]);
    held.forEach((go) => go());
    await until(() => text(view.target).includes("You know Maria Ruiz now.") && text(view.target).includes("You ignored Leo Park."));
  } finally {
    view.destroy();
  }
});

test("the dev fixtures apply each answer to the gate and the brief", () => {
  const ids = (page) => page.senders.map((s) => s.id);
  const owe = () => fx.answer("brief").you_owe.items.map((i) => i.id);
  assert.deepEqual(ids(fx.answer("gate")), ["p-71", "p-72", "p-73"]);
  assert.ok(!owe().includes("c-401"));
  fx.answerSender("p-71", "accept");
  assert.deepEqual(ids(fx.answer("gate")), ["p-72", "p-73"]);
  assert.ok(owe().includes("c-401"), "Maria's ask is under You owe");
  assert.ok(fx.answer("brief/conversations").items.some((i) => i.id === "v-402"));
  assert.ok(!fx.answer("brief").strangers.items.some((i) => i.id === "c-401"));
  fx.answerSender("p-72", "decline");
  assert.deepEqual(ids(fx.answer("gate/declined")), ["p-72", "p-74"]);
  assert.equal(fx.answer("brief").declined, 2);
  // Undo of each answer.
  fx.answerSender("p-71", "reopen");
  fx.answerSender("p-72", "reopen");
  assert.deepEqual(ids(fx.answer("gate")), ["p-71", "p-72", "p-73"]);
  assert.ok(!owe().includes("c-401"));
  assert.equal(fx.answer("brief").declined, 1);
});

test("on the ignored list, Undo of an accept ignores the sender again", async () => {
  const states = { accept: ["person", "person", false], reopen: ["stranger", "engine", false], decline: ["stranger", "engine", true] };
  const asked = serve((path) => {
    const [kind, by, declined] = states[path.split("/")[2]];
    return [200, { ...row("p-74"), kind, decided_by: by, declined }];
  });
  const view = await mount("/src/views/Gate.svelte", { page: fx.ignored, declinedList: true });
  try {
    button(view, "Accept").click();
    await until(() => text(view.target).includes("You know Dana Brooks now."));
    button(view, "Undo").click();
    await until(() => button(view, "Ask again"));
    assert.deepEqual(asked.map(([, p]) => p), ["senders/p-74/accept", "senders/p-74/reopen", "senders/p-74/decline"]);
    assert.ok(button(view, "Accept"));
  } finally {
    view.destroy();
  }
});

test("the brief links all senders while a waiting sender is not on the page, also with answered rows", async () => {
  const many = (from, n) => Array.from({ length: n }, (_, k) => ({ ...fx.gate.senders[1], id: `p-${from + k}`, name: `Sender ${from + k}` }));
  const first = { ...fx.brief, gate: { count: 20, senders: many(100, 20), next: null } };
  // After the accept of p-100: two new senders, 21 wait, and the brief gives the first 20.
  const after = { ...fx.brief, gate: { count: 21, senders: many(101, 20), next: "k" } };
  serve((path) => (path === "brief" ? [200, after] : [200, row("p-100")]));
  const view = await mount("/src/views/Brief.svelte", { watches: [], proposed: null, questions: [], brief: first, now: new Date("2026-11-16T09:05:00Z"), poll: 0 });
  try {
    assert.doesNotMatch(text(view.target), /All 20/);
    buttons(view, "Accept")[0].click();
    await until(() => text(view.target).includes("All 21"));
    assert.match(text(view.target), /You know Sender 100 now\./);
  } finally {
    view.destroy();
  }
});

test("the brief shows the newest load when two loads answer out of order", async () => {
  const held = [];
  const gets = [];
  const afterA = { ...fx.brief, gate: { count: 2, senders: fx.gate.senders.slice(1), next: null } };
  const afterAB = { ...fx.brief, gate: { count: 1, senders: fx.gate.senders.slice(2), next: null } };
  globalThis.fetch = (url) => {
    const path = String(url).replace(/^\/api\//, "");
    if (path !== "brief") return Promise.resolve(new Response(JSON.stringify(rowFor(path)), { status: 200 }));
    gets.push(path);
    const body = gets.length === 1 ? afterA : afterAB;
    return new Promise((done) => held.push(() => done(new Response(JSON.stringify(body), { status: 200 }))));
  };
  const view = await mount("/src/views/Brief.svelte", { watches: [], proposed: null, questions: [], brief: fx.brief, now: new Date("2026-11-16T09:05:00Z"), poll: 0 });
  const heading = () => text(view.target.querySelector("#strangers"));
  try {
    buttons(view, "Accept")[0].click();
    await until(() => gets.length === 1);
    buttons(view, "Ignore")[1].click();
    await until(() => gets.length === 2);
    held[1]();
    await until(() => heading().includes("1"));
    held[0]();
    await new Promise((r) => setTimeout(r, 50));
    view.flushSync();
    assert.match(heading(), /From senders you do not know 1/);
  } finally {
    view.destroy();
  }
});

test("in the brief, a refused answer keeps its row and its words after the brief loads again", async () => {
  const words = "This sender is not at the gate now. Look at the brief again.";
  // A rule took Leo's items out after the brief loaded: the next brief has no Leo.
  const after = { ...fx.brief, gate: { count: 2, senders: [fx.gate.senders[0], fx.gate.senders[2]], next: null } };
  const asked = serve((path) => (path === "brief" ? [200, after] : [409, { error: words }]));
  const view = await mount("/src/views/Brief.svelte", { watches: [], proposed: null, questions: [], brief: fx.brief, now: new Date("2026-11-16T09:05:00Z"), poll: 0 });
  try {
    buttons(view, "Accept")[1].click();
    await until(() => asked.some(([, p]) => p === "brief"));
    await until(() => text(view.target.querySelector("#strangers")).includes("2"));
    view.flushSync();
    assert.match(text(view.target), /Leo Park/);
    assert.match(text(view.target), new RegExp(words.replace(/\./g, "\\.")));
  } finally {
    view.destroy();
  }
});

test("in the brief, a row stays when a new brief comes before the answer", async () => {
  // The engine wrote the accept, and a load of the brief answers first, without Maria.
  const after = { ...fx.brief, gate: { count: 2, senders: fx.gate.senders.slice(1), next: null } };
  let post;
  globalThis.fetch = (url) => {
    const path = String(url).replace(/^\/api\//, "");
    if (path === "brief") return Promise.resolve(new Response(JSON.stringify(after), { status: 200 }));
    if (path === "status") return Promise.resolve(new Response(JSON.stringify(fx.answer("status")), { status: 200 }));
    if (path.startsWith("watches")) return Promise.resolve(new Response(JSON.stringify([]), { status: 200 }));
    if (path === "matters") return Promise.resolve(new Response(JSON.stringify({ proposed: [], confirmed: [] }), { status: 200 }));
    if (path.startsWith("jobs")) return Promise.resolve(new Response(JSON.stringify({ jobs: [], next: null }), { status: 200 }));
    if (path.endsWith("/draft")) return Promise.resolve(new Response(JSON.stringify({ error: "Not found." }), { status: 404 }));
    return new Promise((done) => (post = () => done(new Response(JSON.stringify(rowFor(path)), { status: 200 }))));
  };
  const view = await mount("/src/views/Brief.svelte", { watches: [], proposed: null, questions: [], brief: fx.brief, now: new Date("2026-11-16T09:05:00Z"), poll: 30 });
  try {
    buttons(view, "Accept")[0].click();
    await until(() => post);
    // A poll of the brief comes back while the answer waits.
    await until(() => text(view.target.querySelector("#strangers")).includes("2"));
    view.flushSync();
    assert.match(text(view.target), /Maria Ruiz/);
    post();
    await until(() => text(view.target).includes("You know Maria Ruiz now."));
    assert.ok(button(view, "Undo"));
  } finally {
    view.destroy();
  }
});

// Issue 706: with no answer of the engine, the write can be stored, so the words say that Iris does not know.
test("an answer with no reply of the engine says that Iris does not know if it saved it", async () => {
  globalThis.fetch = async () => {
    throw new TypeError("Failed to fetch");
  };
  const view = await mount("/src/views/Gate.svelte", { page: fx.gate });
  try {
    buttons(view, "Accept")[0].click();
    await until(() => text(view.target).includes("Iris does not know if it saved the answer. Reload the page to see."));
    assert.equal(buttons(view, "Accept").length, 3);
    assert.doesNotMatch(text(view.target), /Try again/);
  } finally {
    view.destroy();
  }
});
