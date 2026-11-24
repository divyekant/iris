// The comment box of the review (card R4-08): a comment on a changeset of a job goes to its agent.
import { test } from "node:test";
import assert from "node:assert/strict";
import { mount, until } from "./mount.mjs";

const act = { n: 1, revision: 2, verb: "archive", targets: {}, resolved: { messages: [] }, args: {}, why: null, flags: [], hash: "h", decision: { state: "approved", by: "person", at: "2026-11-15T09:00:00Z" } };
const changeset = (author) => ({ model: 1, id: "cs-9", revision: 2, author, why: "Tidy the inbox.", created: "2026-11-15T08:00:00Z", submitted: "2026-11-15T08:00:00Z", state: "open", actions: [act] });
const job = { kind: "agent", name: "Claude Code", job: "j-1" };
const old = { id: 4, changeset: "cs-9", revision: 1, text: "Keep the mail from Ana.", at: "2026-11-15T08:30:00Z" };

// `answer(body)` gives [status, body] for a POST of a comment, or throws (no answer).
function serve(answer, list = async () => [200, { comments: [old] }]) {
  const sent = [];
  globalThis.fetch = async (url, init) => {
    const path = String(url).replace(/^\/api\//, "");
    let reply;
    if (path === "changesets/cs-9/comments" && init?.method === "POST") {
      const body = JSON.parse(init.body);
      sent.push(body);
      reply = await answer(body);
    } else if (path === "changesets/cs-9/comments") reply = await list();
    else if (path === "changesets/cs-9") reply = [200, changeset(job)];
    else reply = [404, { error: "Not found." }];
    return new Response(JSON.stringify(reply[1]), { status: reply[0], headers: { "content-type": "application/json" } });
  };
  return sent;
}

// The box opens from one quiet button; it stays open after a send.
async function open(view) {
  const button = await until(() => view.target.querySelector(".comments button.quiet") || view.target.querySelector(".comments textarea"));
  if (button.tagName === "BUTTON") {
    assert.match(button.textContent, /Ask Claude Code for a change/);
    assert.equal(view.target.querySelector(".comments textarea"), null, "no box before the tap");
    button.click();
    view.flushSync();
  }
  return view.target.querySelector(".comments textarea");
}

async function write(view, text) {
  const box = await open(view);
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
    await write(view, "  Do not archive the invoices.\n");
    await until(() => view.target.querySelectorAll(".comments li").length === 2);
    assert.equal(sent.length, 1);
    assert.equal(sent[0].revision, 2, "the comment names the revision on the screen");
    assert.equal(sent[0].text, "  Do not archive the invoices.\n", "the text goes as written");
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
    await write(view, "Keep the invoices.");
    const alert = await until(() => view.target.querySelector(".request > [role=alert]"));
    assert.match(alert.textContent, /did not save your comment: the changeset has a newer revision/);
  } finally {
    view.destroy();
  }
});

// Decision D15: an agent with no job and a local author take a comment; they read it with the request.
test("an agent with no job and a local author have a comment box; triage and you have none", async () => {
  serve(() => [500, {}]);
  for (const [author, box] of [
    [{ kind: "agent", name: "Claude Code" }, true],
    [{ kind: "local", name: "Claude Code" }, true],
    [{ kind: "triage" }, false],
    [{ kind: "person" }, false],
  ]) {
    const view = await mount("/src/views/Request.svelte", { changeset: changeset(author) });
    try {
      if (box) {
        const area = await open(view);
        assert.ok(area, author.kind);
        assert.match(view.target.querySelector(".comments label").textContent, /Claude Code reads it the next time it looks at this request\./);
      } else assert.equal(view.target.querySelector(".comments"), null, author.kind);
    } finally {
      view.destroy();
    }
  }
});

test("a load that answers after a send keeps the new comment", async () => {
  let late;
  const gate = new Promise((r) => (late = r));
  serve((b) => [200, { id: 5, changeset: "cs-9", revision: 2, text: b.text, at: "2026-11-15T09:30:00Z" }], async () => (await gate, [200, { comments: [old] }]));
  const view = await mount("/src/views/Request.svelte", { changeset: changeset(job) });
  try {
    await write(view, "Keep the invoices.");
    await until(() => view.target.querySelector(".comments [role=status]"));
    late();
    await until(() => view.target.querySelectorAll(".comments li").length === 2);
    await new Promise((r) => setTimeout(r, 50));
    assert.deepEqual([...view.target.querySelectorAll(".comments .said")].map((p) => p.textContent), ["Keep the mail from Ana.", "Keep the invoices."]);
  } finally {
    view.destroy();
  }
});

test("the limit counts characters as the engine does, not UTF-16 units", async () => {
  const sent = serve((b) => [200, { id: 5, changeset: "cs-9", revision: 2, text: b.text, at: "2026-11-15T09:30:00Z" }]);
  const view = await mount("/src/views/Request.svelte", { changeset: changeset(job) });
  try {
    const box = await open(view);
    assert.equal(box.getAttribute("maxlength"), null);
    // 4,000 emoji are 8,000 UTF-16 units and 4,000 characters: the engine takes them.
    await write(view, "\u{1F600}".repeat(4000));
    await until(() => sent.length === 1);
    assert.equal([...sent[0].text].length, 4000);
    // One more character is over the limit: the button stays off.
    box.value = "\u{1F600}".repeat(4001);
    box.dispatchEvent(new Event("input", { bubbles: true }));
    view.flushSync();
    const button = view.target.querySelector(".comments button[type=submit]");
    assert.ok(button.disabled);
    assert.match(view.target.querySelector(".comments .count").textContent, /4001 of 4000/);
  } finally {
    view.destroy();
  }
});

test("a comment of only white space is not sent", async () => {
  const sent = serve(() => [200, {}]);
  const view = await mount("/src/views/Request.svelte", { changeset: changeset(job) });
  try {
    await write(view, "  \n ");
    assert.ok(view.target.querySelector(".comments button[type=submit]").disabled);
    assert.equal(sent.length, 0);
  } finally {
    view.destroy();
  }
});

test("the comments stay after the last decision, with no link", async () => {
  serve(() => [200, {}]);
  const view = await mount("/src/views/Request.svelte", { changeset: { ...changeset(job), state: "closed" } });
  try {
    await until(() => view.target.querySelectorAll(".comments li").length === 1);
    assert.equal(view.target.querySelector(".comments button"), null);
    assert.equal(view.target.querySelector(".comments textarea"), null);
  } finally {
    view.destroy();
  }
});

test("after a 409 the same text gets a new key", async () => {
  let n = 0;
  const sent = serve(() => (n++ === 0 ? [409, { error: "the changeset has a newer revision" }] : [200, { id: 9, changeset: "cs-9", revision: 2, text: "Keep it.", at: "2026-11-15T09:30:00Z" }]));
  const view = await mount("/src/views/Request.svelte", { changeset: changeset(job) });
  try {
    await write(view, "Keep it.");
    await until(() => view.target.querySelector(".request > [role=alert]"));
    await until(() => !view.target.querySelector(".comments textarea").disabled);
    await write(view, "Keep it.");
    await until(() => sent.length === 2 && view.target.querySelector(".comments [role=status]"));
    assert.notEqual(sent[1].key, sent[0].key, "the old key names a refused revision");
  } finally {
    view.destroy();
  }
});

test("after an unknown result the comments load again", async () => {
  let saved = false;
  serve(
    (b) => {
      saved = true;
      throw new TypeError(`lost after the save of ${b.text}`);
    },
    async () => [200, { comments: saved ? [old, { id: 5, changeset: "cs-9", revision: 2, text: "Keep it.", at: "2026-11-15T09:30:00Z" }] : [old] }],
  );
  const view = await mount("/src/views/Request.svelte", { changeset: changeset(job) });
  try {
    await write(view, "Keep it.");
    await until(() => view.target.querySelector(".comments [role=alert]"));
    // The engine saved it: the list shows it after the load.
    await until(() => view.target.querySelectorAll(".comments li").length === 2);
  } finally {
    view.destroy();
  }
});

// Decision D15: an author with no job reads the comment with the request, so the confirmation does not promise a session.
test("a comment to an author with no job says when the author reads it", async () => {
  for (const kind of ["agent", "local"]) {
    serve((b) => [200, { id: 5, changeset: "cs-9", revision: 2, text: b.text, at: "2026-11-15T09:30:00Z" }]);
    const view = await mount("/src/views/Request.svelte", { changeset: changeset({ kind, name: "Claude Code" }) });
    try {
      await write(view, "Keep the invoices.");
      const status = await until(() => view.target.querySelector(".comments [role=status]"));
      assert.match(status.textContent, /Claude Code reads it the next time it looks at this request\./, kind);
      assert.doesNotMatch(status.textContent, /session|new revision/, kind);
    } finally {
      view.destroy();
    }
  }
});

// A closed or withdrawn request with no comments shows no empty Comments part; one with comments keeps them.
test("an ended request with no comments has no Comments part", async () => {
  for (const state of ["closed", "withdrawn"]) {
    for (const author of [job, { kind: "agent", name: "Claude Code" }, { kind: "local", name: "Claude Code" }]) {
      let asked = false;
      serve(() => [200, {}], async () => ((asked = true), [200, { comments: [] }]));
      const view = await mount("/src/views/Request.svelte", { changeset: { ...changeset(author), state } });
      try {
        await until(() => asked);
        await new Promise((r) => setTimeout(r, 50));
        view.flushSync();
        assert.equal(view.target.querySelector(".comments"), null, `${state} ${author.kind}`);
      } finally {
        view.destroy();
      }
      serve(() => [200, {}]);
      const kept = await mount("/src/views/Request.svelte", { changeset: { ...changeset(author), state } });
      try {
        await until(() => kept.target.querySelectorAll(".comments li").length === 1);
      } finally {
        kept.destroy();
      }
    }
  }
});
