// Stop (card R4-11): the icon button in the header, the band while stopped, and Resume.
import { test } from "node:test";
import assert from "node:assert/strict";
import { load, mount, until } from "./mount.mjs";

const text = (el) => el.textContent.replace(/\s+/g, " ");

function serve(answer) {
  const asked = [];
  globalThis.fetch = async (url, init) => {
    const path = String(url).replace(/^\/api\//, "");
    asked.push([init?.method ?? "GET", path]);
    const [status, out] = answer(init?.method ?? "GET", path);
    return new Response(JSON.stringify(out), { status, headers: { "content-type": "application/json" } });
  };
  return asked;
}

const since = "2026-11-17T09:00:00Z";

test("Stop is one icon button; only a complete stop says that no mail leaves; Resume runs again", async () => {
  let stop = { stopped: true, since, sessions: 1, live: 1, busy: false };
  const asked = serve((method, path) =>
    path === "resume" ? [200, { stopped: false, since: null, resumed: true }] : method === "POST" ? [200, stop] : [200, { stopped: false, since: null, live: 0, busy: false }],
  );
  const button = await mount("/src/views/Stop.svelte", { place: "button" });
  const band = await mount("/src/views/Stop.svelte", { place: "band" });
  try {
    const stopButton = button.target.querySelector("button.stop-all");
    assert.equal(stopButton.getAttribute("aria-label"), "Stop all agent work");
    assert.equal(text(stopButton).trim(), "", "the button has no text, only the icon");
    stopButton.click();
    await until(() => band.target.querySelector("[role=status]"));
    assert.deepEqual(asked.at(-1), ["POST", "stop"]);
    assert.equal(button.target.querySelector("button.stop-all"), null, "no Stop button while stopped");
    // One session still stops: no claim that no mail leaves.
    assert.match(text(band.target), /The stop is not complete yet/);
    assert.doesNotMatch(text(band.target), /no mail leaves/);
    stop = { ...stop, live: 0 };
    [...band.target.querySelectorAll("button")].find((b) => text(b).includes("Stop again")).click();
    await until(() => /no mail leaves/.test(text(band.target)));
    [...band.target.querySelectorAll("button")].find((b) => text(b).includes("Resume")).click();
    await until(() => button.target.querySelector("button.stop-all"));
    assert.deepEqual(asked.at(-1), ["POST", "resume"]);
    assert.match(text(band.target), /Each held send waits a full minute/);
  } finally {
    button.destroy();
    band.destroy();
  }
});

test("refresh reads a stop that is busy, and the band says that it is not complete", async () => {
  serve(() => [200, { stopped: true, since, live: 0, busy: true }]);
  const band = await mount("/src/views/Stop.svelte", { place: "band" });
  try {
    const mod = await load("/src/views/Stop.svelte");
    await mod.refresh();
    await until(() => band.target.querySelector("[role=status]"));
    assert.match(text(band.target), /The stop is not complete yet/);
    assert.match(text(band.target), /A Gmail change was in progress/);
  } finally {
    band.destroy();
  }
});
