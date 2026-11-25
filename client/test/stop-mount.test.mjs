// Stop (card R4-11): the icon button in the header, the band while stopped, and Resume.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

const text = (el) => el.textContent.replace(/\s+/g, " ");
const since = "2026-11-17T09:00:00Z";
const { act, refresh, reread, stopState, watch } = await load("/src/views/Stop.svelte");

/**
 * A fake engine. `state` is the stop switch. `hold` keeps the answer of a
 * POST (after its commit) until `release` runs; `lose` makes a POST commit
 * with no answer; `reject` refuses a POST with no change; `down` fails each
 * request.
 */
function engine(start = {}) {
  const e = { state: { stopped: false, since: null, live: 0, busy: false, ...start }, posts: [], hold: false, lose: false, reject: false, down: false, waiting: [] };
  e.release = () => e.waiting.splice(0).forEach((r) => r());
  globalThis.fetch = async (url, init) => {
    const path = String(url).replace(/^\/api\//, "");
    const method = init?.method ?? "GET";
    if (e.down) throw new TypeError("Failed to fetch");
    let out = e.state;
    if (method === "POST") {
      e.posts.push(path);
      if (e.reject) return new Response(JSON.stringify({ error: "database: locked" }), { status: 500, headers: { "content-type": "application/json" } });
      const resumed = path === "resume" && e.state.stopped;
      e.state = path === "stop" ? { ...e.state, stopped: true, since: e.state.since ?? since } : { stopped: false, since: null, live: 0, busy: false };
      out = path === "resume" ? { stopped: false, since: null, resumed } : { ...e.state, sessions: 0 };
      if (e.hold) await new Promise((r) => e.waiting.push(r));
      if (e.lose) throw new TypeError("Failed to fetch");
    }
    return new Response(JSON.stringify(out), { status: 200, headers: { "content-type": "application/json" } });
  };
  return e;
}

async function start(e) {
  Object.assign(stopState, { busyAction: "", note: "", error: "" });
  await reread();
  const button = await mount("/src/views/Stop.svelte", { place: "button" });
  const band = await mount("/src/views/Stop.svelte", { place: "band" });
  const flush = () => (button.flushSync(), band.flushSync());
  return { button, band, flush, e, done: () => (button.destroy(), band.destroy()) };
}
const press = (view, name) => [...view.target.querySelectorAll("button")].find((b) => text(b).includes(name)).click();

test("Stop is one icon button; only a complete stop says that no mail leaves; Resume runs again", async () => {
  const v = await start(engine());
  try {
    const stopButton = v.button.target.querySelector("button.stop-all");
    assert.equal(stopButton.getAttribute("aria-label"), "Stop all agent work");
    assert.equal(text(stopButton).trim(), "", "the button has no text, only the icon");
    assert.doesNotMatch(stopButton.getAttribute("title"), /mail/);
    v.e.state.live = 1; // One session still stops after the Stop.
    stopButton.click();
    await until(() => v.band.target.querySelector('[aria-label="Iris is stopped"]'));
    assert.deepEqual(v.e.posts, ["stop"]);
    assert.equal(v.button.target.querySelector("button.stop-all"), null, "no Stop button while stopped");
    assert.match(text(v.band.target), /The stop is not complete yet/);
    assert.doesNotMatch(text(v.band.target), /no mail leaves/);
    v.e.state.live = 0;
    press(v.band, "Stop again");
    await until(() => /no mail leaves/.test(text(v.band.target)));
    press(v.band, "Resume");
    await until(() => v.button.target.querySelector("button.stop-all"));
    assert.deepEqual(v.e.posts, ["stop", "stop", "resume"]);
    await until(() => /Each held send waits a full minute/.test(text(v.band.target)));
  } finally {
    v.done();
  }
});

test("a busy stop is not complete, and offers Stop again", async () => {
  const v = await start(engine({ stopped: true, since, busy: true }));
  try {
    assert.match(text(v.band.target), /The stop is not complete yet/);
    assert.match(text(v.band.target), /A Gmail change was in progress/);
    assert.ok([...v.band.target.querySelectorAll("button")].some((b) => text(b).includes("Stop again")));
  } finally {
    v.done();
  }
});

test("while a Resume waits for its answer, the band makes no claim and Stop stays possible", async () => {
  const v = await start(engine({ stopped: true, since }));
  try {
    assert.match(text(v.band.target), /no mail leaves/);
    // The state verbs of an item still work while Iris is stopped, and they change no mailbox, so the band names only mailbox changes (#596).
    assert.match(text(v.band.target), /No agent runs, no mailbox change applies and no mail leaves\./);
    v.e.hold = true;
    press(v.band, "Resume");
    await until(() => v.e.waiting.length === 1); // The Resume committed; its answer waits.
    v.flush();
    assert.doesNotMatch(text(v.band.target), /no mail leaves|Iris is stopped/);
    const stopButton = v.button.target.querySelector("button.stop-all");
    assert.equal(stopButton.disabled, false);
    // A second Resume sends nothing.
    await Promise.race([act("resume"), new Promise((r) => setTimeout(r, 20))]);
    assert.deepEqual(v.e.posts, ["resume"]);
    // The Stop commits after the Resume. Both answers come in the other order.
    stopButton.click();
    await until(() => v.e.waiting.length === 2);
    v.e.release();
    await until(() => stopState.known);
    v.flush();
    assert.equal(stopState.stopped, true);
    assert.match(text(v.band.target), /no mail leaves/);
  } finally {
    v.e.release();
    v.done();
  }
});

test("a Stop that commits after a Resume answers wins, and the screen shows the stop", async () => {
  const v = await start(engine({ stopped: true, since }));
  try {
    v.e.hold = true;
    press(v.band, "Resume");
    await until(() => v.e.waiting.length === 1);
    v.button.target.querySelector("button.stop-all").click();
    await until(() => v.e.waiting.length === 2);
    // The Resume answers first, then the Stop.
    v.e.waiting.shift()();
    await new Promise((r) => setTimeout(r, 20));
    v.flush();
    assert.equal(stopState.known, false, "the Stop is still open");
    v.e.release();
    await until(() => stopState.known);
    assert.equal(stopState.stopped, true);
  } finally {
    v.e.release();
    v.done();
  }
});

test("a Resume with a lost answer: no claim until a read shows that Iris runs", async () => {
  const v = await start(engine({ stopped: true, since }));
  try {
    v.e.lose = true;
    press(v.band, "Resume");
    // The read after the lost answer shows that the Resume committed.
    await until(() => stopState.known && !stopState.stopped);
    assert.equal(stopState.error, "", "no error to try again a completed action");
  } finally {
    v.done();
  }
});

test("a Stop with a lost answer and a down engine: no claim, with Stop again and Check again", async () => {
  const v = await start(engine());
  try {
    v.e.lose = true;
    v.e.hold = true;
    v.button.target.querySelector("button.stop-all").click();
    await until(() => v.e.waiting.length === 1); // The Stop committed.
    v.e.down = true;
    v.e.release();
    await until(() => stopState.error && !stopState.busyAction);
    v.flush();
    assert.equal(stopState.known, false);
    assert.doesNotMatch(text(v.band.target), /no mail leaves|Iris is stopped/);
    assert.match(text(v.band.target), /does not know if agent work is stopped/);
    assert.ok([...v.band.target.querySelectorAll("button")].some((b) => text(b).includes("Stop again")));
    v.e.down = false;
    press(v.band, "Check again");
    await until(() => stopState.known);
    assert.equal(stopState.stopped, true);
    assert.equal(stopState.error, "");
  } finally {
    v.done();
  }
});

test("a rejected Stop keeps its error, and the state stays not known", async () => {
  const v = await start(engine());
  try {
    v.e.reject = true;
    await act("stop");
    v.flush();
    assert.equal(stopState.known, false);
    assert.notEqual(stopState.error, "");
    assert.ok(v.band.target.querySelector("[role=alert]"));
    assert.doesNotMatch(text(v.band.target), /no mail leaves/);
  } finally {
    v.done();
  }
});

test("a failed read after a complete stop takes back the claim, and the Stop button comes back", async () => {
  const v = await start(engine({ stopped: true, since }));
  try {
    assert.match(text(v.band.target), /no mail leaves/);
    v.e.down = true;
    await refresh();
    v.flush();
    assert.doesNotMatch(text(v.band.target), /no mail leaves|Iris is stopped/);
    assert.ok(v.button.target.querySelector("button.stop-all"));
  } finally {
    v.done();
  }
});

test("an older read never replaces a newer state", async () => {
  const v = await start(engine({ stopped: true, since }));
  try {
    v.e.hold = false;
    let release;
    const late = new Promise((r) => (release = r));
    const real = globalThis.fetch;
    globalThis.fetch = async (url, init) => ((init?.method ?? "GET") === "GET" && !release.used ? ((release.used = true), await late, new Response(JSON.stringify({ stopped: true, since, live: 0, busy: false }), { status: 200 })) : real(url, init));
    const reading = refresh();
    await act("resume");
    release();
    await reading;
    assert.equal(stopState.stopped, false, "the late read did not stop Iris again on the screen");
    globalThis.fetch = real;
  } finally {
    v.done();
  }
});

test("a Resume when Iris already runs does not say that a new hold started", async () => {
  const v = await start(engine());
  try {
    await act("resume");
    assert.equal(stopState.stopped, false);
    assert.equal(stopState.note, "");
  } finally {
    v.done();
  }
});

test("a stop or a resume at the terminal shows in an open tab with no new view or focus (issue 589)", async () => {
  const e = engine();
  const v = await start(e);
  const end = watch(20);
  try {
    assert.equal(v.band.target.querySelector("[aria-label='Iris is stopped']"), null);
    // `iris stop` at the terminal.
    e.state = { stopped: true, since, live: 0, busy: false };
    await until(() => (v.flush(), v.band.target.querySelector("[aria-label='Iris is stopped']")));
    // `iris resume` at the terminal.
    e.state = { stopped: false, since: null, live: 0, busy: false };
    await until(() => (v.flush(), !v.band.target.querySelector("[aria-label='Iris is stopped']")));
  } finally {
    end();
    v.done();
  }
});
