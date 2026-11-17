// "Handle this" (R4-10) in a DOM: the link on the item page, the start page,
// the body that it sends, and the answers of the engine.
process.env.TZ = "America/Los_Angeles";
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

// Start.svelte opens the job through location.hash.
globalThis.location = window.location;
const fx = await load("/dev/fixtures.ts");
const s = await load("/src/start.ts");
const { parse, href } = await load("/src/route.ts");
const commitment = (status = "open") => ({ ...fx.items.find((i) => i.id === "c-311"), status });
const conversation = (status = "active") => ({ ...fx.items.find((i) => i.id === "v-88"), status });
const text = (el) => el.textContent.replace(/\s+/g, " ");
const button = (view, name) => [...view.target.querySelectorAll("button")].find((b) => text(b).trim().startsWith(name));

// Answer each request with `answer(method, path, body)`: [status, body].
function serve(answer) {
  const asked = [];
  globalThis.fetch = async (url, init) => {
    const path = String(url).replace(/^\/api\//, "");
    const method = init?.method ?? "GET";
    const body = init?.body ? JSON.parse(init.body) : undefined;
    asked.push([method, path, body]);
    const [status, out] = await answer(method, path, body);
    return new Response(JSON.stringify(out), { status, headers: { "content-type": "application/json" } });
  };
  return asked;
}
const preview = (id) => fx.answer(`/items/${id}/job`);
const type = (view, value) => {
  const area = view.target.querySelector("textarea");
  area.value = value;
  area.dispatchEvent(new window.Event("input", { bubbles: true }));
  view.flushSync();
};

const handle = (view) => view.target.querySelector("a.handle");

test("Handle this shows only on an item that can start a job, and links to its start page", async () => {
  serve(() => [200, {}]);
  for (const [item, shows] of [
    [commitment("open"), true],
    [commitment("done"), false],
    [commitment("deferred"), false],
    [conversation("active"), true],
    [conversation("quiet"), true],
    [conversation("muted"), false],
  ]) {
    const view = await mount("/src/views/Item.svelte", { item });
    assert.equal(!!handle(view), shows, `${item.form} ${item.status}`);
    if (shows) assert.equal(handle(view).getAttribute("href"), `#/item/${item.id}/job`);
    view.destroy();
  }
  assert.deepEqual(parse("#/item/c-311/job"), { view: "start", id: "c-311" });
  assert.deepEqual(parse("#/item/c-311"), { view: "item", id: "c-311" });
  for (const bad of ["#/item/c-311/job/x", "#/item/c-311/jobs", "#/mail/m-1/job"]) assert.deepEqual(parse(bad), { view: "home" }, bad);
  assert.equal(parse(href.start("c-311")).view, "start");
});

test("the start page shows the item, an empty goal and each message of the grant; Cancel goes back to the item", async () => {
  serve(() => [200, preview("c-311")]);
  location.hash = "";
  const view = await mount("/src/views/StartPage.svelte", { item: commitment() });
  await until(() => view.target.querySelector(".thread"));
  assert.match(text(view.target), /Give this to an agent/);
  assert.match(text(view.target), /Review contract section 4/);
  assert.equal(view.target.querySelector("textarea").value, "", "no text of the mail fills the goal");
  assert.match(text(view.target), /The agent reads 3 messages in 1 thread, and no other mail\./);
  assert.match(text(view.target), /Hidden from the agent for now/);
  assert.ok(button(view, "Start the job").disabled, "no start with an empty goal");
  button(view, "Cancel").click();
  assert.equal(location.hash, "#/item/c-311");
  view.destroy();
});

test("Start the job sends the goal, the due instant and the IDs that the person saw, then opens the job", async () => {
  const asked = serve((method, path) => (method === "GET" ? [200, preview("c-311")] : [201, fx.jobs["j-12"]]));
  location.hash = "";
  const item = commitment();
  const view = await mount("/src/views/Start.svelte", { item });
  await until(() => view.target.querySelector(".thread"));
  type(view, "  Send Priya my comments.  ");
  button(view, "Start the job").click();
  await until(() => location.hash === "#/jobs/j-12");
  const [, path, body] = asked.find(([m]) => m === "POST");
  assert.equal(path, "jobs");
  assert.deepEqual(body, {
    item: "c-311",
    goal: "Send Priya my comments.",
    due: s.dueValue(item, s.dueDate(item)),
    messages: preview("c-311").messages.map((m) => m.id),
  });
  assert.match(body.due, /T23:59:59\.999-0[78]:00$/, "a date-only commitment is due at the end of its local day");
  view.destroy();
});

test("a 409 with a job opens that job; another 409 shows the reason and loads the mail again", async () => {
  let reply = [409, { error: "A job that is not done has this item already.", job: "j-14" }];
  const asked = serve((method) => (method === "GET" ? [200, preview("c-311")] : reply));
  location.hash = "";
  let view = await mount("/src/views/Start.svelte", { item: commitment() });
  await until(() => view.target.querySelector(".thread"));
  type(view, "Go.");
  button(view, "Start the job").click();
  await until(() => location.hash === "#/jobs/j-14");
  view.destroy();

  reply = [409, { error: "The mail changed. Look at it again." }];
  location.hash = "";
  asked.length = 0;
  view = await mount("/src/views/Start.svelte", { item: commitment() });
  await until(() => view.target.querySelector(".thread"));
  type(view, "Go.");
  button(view, "Start the job").click();
  await until(() => view.target.querySelector("[role=alert]"));
  assert.match(text(view.target.querySelector("[role=alert]")), /The mail changed/);
  await until(() => asked.filter(([m]) => m === "GET").length === 2);
  assert.equal(location.hash, "", "no job opens");
  view.destroy();
});

test("each message shows its subject when it is not the subject of its thread", async () => {
  const base = preview("c-311").messages;
  const list = { messages: [base[0], { ...base[1], subject: "Re: Q4 numbers for the board" }, { ...base[2], subject: "Salary review, private" }, { ...base[2], id: "m-1", subject: null }] };
  serve(() => [200, list]);
  const view = await mount("/src/views/Start.svelte", { item: commitment() });
  await until(() => view.target.querySelector(".thread"));
  const rows = [...view.target.querySelectorAll("li")].map(text);
  assert.equal(rows.length, 4);
  assert.match(rows[1], /Re: Q4 numbers for the board/);
  assert.match(rows[2], /Salary review, private/);
  assert.match(rows[3], /\(no subject\)/);
  assert.doesNotMatch(rows[0], /Q4 numbers/, "the heading names the subject of the first message");
  view.destroy();
});

test("an item that closed after the preview: the page shows the item as it is now, and the reason", async () => {
  const asked = serve((method, path) => {
    if (method === "POST") return [409, { error: "The item is not open any more." }];
    if (path.endsWith("/job")) return asked.some(([m]) => m === "POST") ? [409, { error: "The item is not open any more." }] : [200, preview("c-311")];
    return [200, commitment("done")];
  });
  location.hash = "";
  const view = await mount("/src/views/StartPage.svelte", { item: commitment() });
  await until(() => view.target.querySelector(".thread"));
  type(view, "Go.");
  button(view, "Start the job").click();
  await until(() => asked.some(([m, p]) => m === "GET" && p === "items/c-311"));
  await until(() => view.target.querySelector("[role=alert]") && (view.flushSync(), button(view, "Start the job").disabled));
  assert.match(text(view.target), /The item is not open any more\./);
  assert.ok(button(view, "Start the job").disabled, "no second start from a closed item");
  assert.equal(location.hash, "");
  view.destroy();
});

test("after a 409, the due field takes the new due date of the item, unless the person changed the field", async () => {
  for (const edit of [false, true]) {
    let posts = 0;
    const asked = serve((method, path) => {
      if (method === "POST") return ++posts === 1 ? [409, { error: "The mail changed. Look at it again." }] : [201, fx.jobs["j-12"]];
      if (path.endsWith("/job")) return [200, preview("c-311")];
      return [200, { ...commitment(), due: { date: "2026-11-24", strict: false } }];
    });
    location.hash = "";
    const view = await mount("/src/views/StartPage.svelte", { item: commitment() });
    await until(() => view.target.querySelector(".thread"));
    if (edit) {
      const field = view.target.querySelector("input[type=date]");
      field.value = "2026-11-19";
      field.dispatchEvent(new window.Event("input", { bubbles: true }));
      view.flushSync();
    }
    type(view, "Go.");
    button(view, "Start the job").click();
    await until(() => view.target.querySelector("[role=alert]"));
    await until(() => asked.filter(([m, p]) => m === "GET" && p.endsWith("/job")).length >= 2);
    await until(() => (view.flushSync(), !button(view, "Start the job").disabled));
    const want = edit ? "2026-11-19" : "2026-11-24";
    assert.equal(view.target.querySelector("input[type=date]").value, want, `edit ${edit}`);
    button(view, "Start the job").click();
    await until(() => location.hash === "#/jobs/j-12");
    assert.equal(asked.filter(([m]) => m === "POST")[1][2].due, s.endOfDay(want), `edit ${edit}`);
    view.destroy();
  }
});
