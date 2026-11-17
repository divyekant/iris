// The comment box of the review (card R4-08): a comment on a changeset of a job goes to its agent.
import { test } from "node:test";
import assert from "node:assert/strict";
import { mount, until } from "./mount.mjs";

const act = { n: 1, revision: 2, verb: "archive", targets: {}, resolved: { messages: [] }, args: {}, why: null, flags: [], hash: "h", decision: { state: "approved", by: "person", at: "2026-11-15T09:00:00Z" } };
const changeset = (author) => ({ model: 1, id: "cs-9", revision: 2, author, why: "Tidy the inbox.", created: "2026-11-15T08:00:00Z", submitted: "2026-11-15T08:00:00Z", state: "open", actions: [act] });
const job = { kind: "agent", name: "Claude Code", job: "j-1" };
const old = { id: 4, changeset: "cs-9", revision: 1, text: "Keep the mail from Ana.", at: "2026-11-15T08:30:00Z" };

// `answer(body)` gives [status, body] for a POST of a comment, or throws (no answer).
function serve(answer) {
  const sent = [];
  globalThis.fetch = async (url, init) => {
    const path = String(url).replace(/^\/api\//, "");
    let reply;
    if (path === "changesets/cs-9/comments" && init?.method === "POST") {
      const body = JSON.parse(init.body);
      sent.push(body);
      reply = await answer(body);
    } else if (path === "changesets/cs-9/comments") reply = [200, { comments: [old] }];
    else if (path === "changesets/cs-9") reply = [200, changeset(job)];
    else reply = [404, { error: "Not found." }];
    return new Response(JSON.stringify(reply[1]), { status: reply[0], headers: { "content-type": "application/json" } });
  };
  return sent;
}

async function write(view, text) {
  const box = view.target.querySelector(".comments textarea");
  box.value = text;
  box.dispatchEvent(new Event("input", { bubbles: true }));
  view.flushSync();
  view.target.querySelector(".comments form").dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
}

test("a changeset of a job shows its comments, and a comment goes to the agent", async () => {
  const sent = serve((b) => [200, { id: 5, changeset: "cs-9", revision: b.revision, text: b.text, at: "2026-11-15T09:30:00Z" }]);
  const view = await mount("/src/views/Request.svelte", { changeset: changeset(job) });
  try {
    await until(() => view.target.querySelectorAll(".comments li").length === 1);
    assert.match(view.target.querySelector(".comments li").textContent, /revision 1[\s\S]*Keep the mail from Ana\./);
    await write(view, "  Do not archive the invoices.  ");
    await until(() => view.target.querySelectorAll(".comments li").length === 2);
    assert.equal(sent.length, 1);
    assert.equal(sent[0].revision, 2, "the comment names the revision on the screen");
    assert.equal(sent[0].text, "Do not archive the invoices.");
    assert.match(sent[0].key, /^[A-Za-z0-9_-]{16,64}$/);
    assert.match(view.target.querySelector(".comments [role=status]").textContent, /Claude Code/);
    assert.equal(view.target.querySelector(".comments textarea").value, "");
  } finally {
    view.destroy();
  }
});

test("a send again after an unknown result sends the same key; a new text gets a new key", async () => {
  let n = 0;
  const sent = serve((b) => {
    if (n++ === 0) throw new TypeError("Failed to fetch");
    return [200, { id: 5 + n, changeset: "cs-9", revision: 2, text: b.text, at: "2026-11-15T09:30:00Z" }];
  });
  const view = await mount("/src/views/Request.svelte", { changeset: changeset(job) });
  try {
    await until(() => view.target.querySelector(".comments textarea"));
    await write(view, "Keep the invoices.");
    const alert = await until(() => view.target.querySelector(".comments [role=alert]"));
    assert.match(alert.textContent, /does not know if it saved/);
    // The text stays, so that the person can send it again.
    assert.equal(view.target.querySelector(".comments textarea").value, "Keep the invoices.");
    await write(view, "Keep the invoices.");
    await until(() => sent.length === 2 && view.target.querySelector(".comments [role=status]"));
    assert.equal(sent[1].key, sent[0].key);
    await write(view, "And the receipts.");
    await until(() => sent.length === 3);
    assert.notEqual(sent[2].key, sent[0].key);
  } finally {
    view.destroy();
  }
});

test("a refused comment loads the request again and says why", async () => {
  serve(() => [409, { error: "the changeset has a newer revision" }]);
  const view = await mount("/src/views/Request.svelte", { changeset: changeset(job) });
  try {
    await until(() => view.target.querySelector(".comments textarea"));
    await write(view, "Keep the invoices.");
    const alert = await until(() => view.target.querySelector(".request > [role=alert]"));
    assert.match(alert.textContent, /did not save your comment: the changeset has a newer revision/);
  } finally {
    view.destroy();
  }
});

test("a changeset with no job has no comment box", async () => {
  serve(() => [500, {}]);
  for (const author of [{ kind: "agent", name: "Claude Code" }, { kind: "triage" }]) {
    const view = await mount("/src/views/Request.svelte", { changeset: changeset(author) });
    try {
      assert.equal(view.target.querySelector(".comments"), null);
    } finally {
      view.destroy();
    }
  }
});
