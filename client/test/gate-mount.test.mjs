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
    asked.push([init?.method ?? "GET", path]);
    const [status, out] = await answer(path);
    return new Response(JSON.stringify(out), { status, headers: { "content-type": "application/json" } });
  };
  return asked;
}

const row = (id) => ({ id, address: "a@b.example", name: null, kind: "person", decided_by: "person", declined: false });

test("Accept answers the sender, and Undo asks again", async () => {
  const asked = serve((path) => [200, row(path.split("/")[1])]);
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
  const asked = serve((path) => [200, row(path.split("/")[1])]);
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
  const view = await mount("/src/views/Brief.svelte", { brief: fx.brief, now: new Date("2026-11-16T09:05:00Z"), poll: 0 });
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
  const view = await mount("/src/views/Brief.svelte", { brief: only, now: new Date("2026-11-16T09:05:00Z"), poll: 0 });
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
    asked.push(String(url).replace(/^\/api\//, ""));
    return new Promise((done) => held.push(() => done(new Response(JSON.stringify(row("x")), { status: 200 }))));
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
