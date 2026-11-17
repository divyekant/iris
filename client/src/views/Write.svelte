<!--
  The write view (docs/specs/2026-11-17-write.md, section 6), layout A
  "Letter": a quiet sheet with one line for the recipients, the subject as a
  title and the text as the page. A reply shows the mail that it answers,
  short, above the sheet. One press of Send writes the mail: the engine makes
  the draft, then the send, which waits in the outbox for its hold.

  The form content and its key stay together in sessionStorage until the
  engine takes the mail or the person discards it, so a reload or another
  view keeps both. One key belongs to one content: a press after a lost
  answer sends the same key, and the engine makes the mail only once.

  Mail text is data: Svelte escapes the text of the parent.
-->
<script lang="ts">
  import { onDestroy, untrack } from "svelte";
  import { get, own, post, type Account, type ReplyStart, type WriteState } from "../api";
  import { chips, unique } from "../chips";
  import { initials, when } from "../format";
  import { FLAGS } from "../review";
  import { href } from "../route";

  type Start = { accounts: Account[] } | { thread: string; reply: ReplyStart };
  // `every`: the ms between two reads of the write state (a prop for the tests).
  let { start, every = 2000 }: { start: Start; every?: number } = $props();

  // svelte-ignore state_referenced_locally
  const reply = "reply" in start ? start.reply : null;
  // svelte-ignore state_referenced_locally
  const thread = "thread" in start ? start.thread : null;
  // svelte-ignore state_referenced_locally
  const accounts = "accounts" in start ? start.accounts : [];
  const place = `iris.write.${thread ?? "new"}`;

  type Saved = {
    account: string;
    to: string[];
    cc: string[];
    subject: string;
    body: string;
    key: string;
    sig: string;
    flags: string[];
    toText?: string;
    ccText?: string;
  };
  function storage(): Storage | undefined {
    try {
      return globalThis.sessionStorage;
    } catch {
      return undefined;
    }
  }
  function restore(): Saved | null {
    try {
      const v = JSON.parse(storage()?.getItem(place) ?? "null");
      return v && typeof v.key === "string" && Array.isArray(v.to) ? (v as Saved) : null;
    } catch {
      return null;
    }
  }
  // Remove the saved form only if it is still the form of the press with `sent`:
  // a late answer must not remove a newer form of the same place.
  const forget = (sent?: string) => {
    try {
      if (sent === undefined || restore()?.key === sent) storage()?.removeItem(place);
    } catch {
      // The browser blocks the storage: nothing to remove.
    }
  };

  const saved = restore();
  let account = $state(saved?.account ?? reply?.account ?? (accounts[0] ? `a-${accounts[0].id}` : ""));
  let to = $state<string[]>(unique(saved?.to ?? reply?.to ?? []));
  const startCc = unique(saved?.cc ?? reply?.cc ?? []);
  let cc = $state<string[]>(startCc);
  let subject = $state(saved?.subject ?? "");
  let body = $state(saved?.body ?? "");
  let copy = $state(startCc.length > 0);
  let toText = $state(saved?.toText ?? "");
  let ccText = $state(saved?.ccText ?? "");
  // The key of the press and the content that it belongs to. A new content gets a new key.
  let key = $state(saved?.key ?? "");
  let sig = $state(saved?.sig ?? "");
  // The warnings that the engine named for this content (a 409). The next press sends them.
  let flags = $state<string[]>(saved?.flags ?? []);
  let problem = $state("");
  let unknown = $state(false);
  let busy = $state(false);

  // The parent is part of the content: a reply to a new parent is a new mail.
  let content = $derived(JSON.stringify([thread ? [thread, reply?.parent.id] : account, to, cc, thread ? "" : subject, body]));
  $effect(() => {
    const now = content;
    untrack(() => {
      if (now !== sig) {
        sig = now;
        key = globalThis.crypto.randomUUID();
        flags = [];
        unknown = false;
      }
    });
  });
  // Save each part of the form that a reload must keep: the text of an address
  // not yet a chip, and the warnings that the next press sends, too.
  $effect(() => {
    const form: Saved = { account, to, cc, subject, body, key, sig, flags, toText, ccText };
    if (write) return;
    try {
      storage()?.setItem(place, JSON.stringify(form));
    } catch {
      // With no storage, the form lives until the page closes.
    }
  });

  const ADDRESS = /^[^\s@,<>]+@[^\s@,<>]+\.[^\s@,<>]+$/;
  const valid = (a: string) => ADDRESS.test(a);
  let ready = $derived(to.some(valid) && (thread !== null || subject.trim() !== "") && !busy);

  function typed(field: "to" | "cc", end = false) {
    const [list, rest] = chips(field === "to" ? to : cc, field === "to" ? toText : ccText, end);
    if (field === "to") [to, toText] = [list, rest];
    else [cc, ccText] = [list, rest];
  }
  function keydown(field: "to" | "cc", e: KeyboardEvent) {
    const text = field === "to" ? toText : ccText;
    if (e.key === "Enter") {
      e.preventDefault();
      typed(field, true);
    } else if (e.key === "Backspace" && text === "") {
      if (field === "to") to = to.slice(0, -1);
      else cc = cc.slice(0, -1);
    }
  }
  const remove = (field: "to" | "cc", a: string) => (field === "to" ? (to = to.filter((x: string) => x !== a)) : (cc = cc.filter((x: string) => x !== a)));

  // After the engine took the mail: the write `cs-N` and its state.
  let write = $state("");
  let ws = $state<WriteState>({ state: "saving" });
  let now = $state(Date.now());
  let note = $state("");

  async function send() {
    typed("to", true);
    typed("cc", true);
    if (!ready) return;
    busy = true;
    problem = "";
    const target = thread ? { thread, parent: reply?.parent.id } : { account, subject };
    const sent = key;
    try {
      const v = await post<{ write: string }>("write", { ...target, to, cc, body, key, flags });
      forget(sent);
      if (!alive) return;
      unknown = false;
      write = v.write;
      follow();
    } catch (e) {
      const status = (e as { status?: number }).status;
      const answer = (e as { body?: { flags?: string[] } }).body;
      if (!status || status >= 500) {
        // No answer: the engine may have the mail. The next press uses the same key.
        unknown = true;
      } else {
        problem = own(e);
        if (status === 409 && answer?.flags?.length) flags = answer.flags;
      }
    }
    busy = false;
  }

  function discard() {
    forget();
    location.hash = reply ? href.mail(reply.parent.id) : "#/brief";
  }

  // The reads of the state: one at a time, `every` ms after the end of the last one.
  // Each read gets a number when it starts, and only a read newer than the last applied one counts.
  let started = 0;
  let applied = 0;
  let alive = true;
  let timer: ReturnType<typeof setTimeout> | undefined;
  async function read() {
    const mine = ++started;
    const id = write;
    try {
      const v = await get<WriteState>(`write/${encodeURIComponent(id)}`);
      // A read of an earlier write (before Write another) does not count.
      if (mine <= applied || id !== write) return;
      applied = mine;
      ws = v;
    } catch {
      // The engine can restart. The next read tries again.
    }
  }
  const open = () => ws.state === "saving" || ws.state === "waiting";
  function follow() {
    const id = write;
    clearTimeout(timer);
    const loop = async () => {
      await read();
      if (alive && id === write && open()) timer = setTimeout(loop, every);
    };
    void loop();
  }
  const tick = setInterval(() => (now = Date.now()), 1000);
  onDestroy(() => {
    alive = false;
    clearTimeout(timer);
    clearInterval(tick);
  });

  function left(at: string | null): string | null {
    if (!at) return null;
    const sec = Math.ceil((Date.parse(at) - now) / 1000);
    return sec > 0 ? `${Math.floor(sec / 60)}:${String(sec % 60).padStart(2, "0")}` : null;
  }
  const gone = (at: string) => Math.min(1, Math.max(0, 1 - (Date.parse(at) - now) / 60_000));

  async function stop() {
    busy = true;
    note = "";
    const waiting = ws.state === "waiting";
    try {
      if (ws.state === "waiting") await post(`outbox/${encodeURIComponent(ws.draft)}/cancel`, {});
      else await post(`changesets/${encodeURIComponent(write)}/withdraw`, {});
    } catch (e) {
      // 409: the engine sends the mail now, or sent it, so a stop cannot hold.
      if ((e as { status?: number }).status !== 409) note = own(e);
      else note = waiting ? "The send started. It cannot be cancelled." : "The mail left or is leaving now. Iris cannot stop it.";
    }
    await read();
    busy = false;
  }

  function again() {
    if (thread) return void (location.hash = "#/write");
    clearTimeout(timer);
    [write, to, cc, subject, body, note, toText, ccText] = ["", [], [], "", "", "", "", ""];
    ws = { state: "saving" };
    copy = false;
  }

  const who = $derived(to[0] ?? "");
</script>

{#if write}
  <section class="done" aria-live="polite">
    {#if ws.state === "saving"}
      <span class="ring" aria-hidden="true">…</span>
      <h1>Saving your mail</h1>
      <p class="muted">Iris puts the mail in your Gmail drafts, then in the Outbox.</p>
      <div class="row"><button type="button" class="pill" onclick={stop} disabled={busy}>Stop this mail</button></div>
    {:else if ws.state === "waiting"}
      <span class="ring" aria-hidden="true">✓</span>
      <h1>On its way to {who}</h1>
      {#if ws.sending}
        <p class="muted">It waits 60 seconds in the Outbox before it leaves, so you can still change your mind.</p>
        {#if ws.leaves_at}
          <span class="bar" aria-hidden="true"><span style:width="{gone(ws.leaves_at) * 100}%"></span></span>
          <p class="small">{left(ws.leaves_at) ? `Leaves in ${left(ws.leaves_at)}` : "Leaves now"}</p>
        {/if}
      {:else}
        <p class="off"><strong>Sending is off. The mail waits in the Outbox.</strong> To turn it on, run <code>iris sending on</code> at the terminal.</p>
      {/if}
      <div class="row">
        <button type="button" class="pill" onclick={stop} disabled={busy}>Cancel the send</button>
        <button type="button" class="quiet" onclick={again}>Write another</button>
      </div>
    {:else if ws.state === "review"}
      <span class="ring warn" aria-hidden="true">!</span>
      <h1>Look at this mail before it leaves</h1>
      <p class="muted">{ws.reason ?? "Iris put the send on the review screen."}</p>
      <div class="row"><a class="pill" href="#/review">Open Review</a></div>
    {:else if ws.state === "sent"}
      <span class="ring" aria-hidden="true">✓</span>
      <h1>Sent to {who}</h1>
      <div class="row"><button type="button" class="quiet" onclick={again}>Write another</button></div>
    {:else}
      <span class="ring stop" aria-hidden="true">×</span>
      <h1>This mail did not leave</h1>
      <p class="muted">{ws.reason}</p>
      <div class="row"><button type="button" class="quiet" onclick={again}>Write another</button></div>
    {/if}
    {#if note}<p role="alert">{note}</p>{/if}
  </section>
{:else}
  {#if reply}
    <div class="parent">
      <span class="mark" aria-hidden="true">{initials({ kind: "sender", name: reply.parent.from.name ?? undefined, address: reply.parent.from.address ?? undefined })}</span>
      <div>
        <p><span class="from">{reply.parent.from.name || reply.parent.from.address || "Unknown sender"}</span>{#if reply.parent.date}<span class="faint"> · {when(reply.parent.date)}</span>{/if}</p>
        {#if reply.parent.text}<p class="text">{reply.parent.text}</p>{/if}
      </div>
    </div>
  {:else}
    <h1 class="title">New mail</h1>
  {/if}

  <form class="sheet" onsubmit={(e) => (e.preventDefault(), send())}>
    {#if accounts.length > 1}
      <label class="line">
        <span class="k">From</span>
        <select bind:value={account}>{#each accounts as a (a.id)}<option value={`a-${a.id}`}>{a.address}</option>{/each}</select>
      </label>
    {/if}
    {#each [["to", "To"], ["cc", "Copy"]] as const as [field, label] (field)}
      {#if field === "to" || copy}
        <div class="line">
          <label class="k" for="write-{field}">{label}</label>
          {#each field === "to" ? to : cc as a (a)}
            <span class="chip" class:bad={!valid(a)} title={valid(a) ? undefined : "This address does not look valid"}>
              {a}<button type="button" aria-label="Remove {a}" onclick={() => remove(field, a)}>×</button>
            </span>
          {/each}
          {#if field === "to"}
            <input id="write-to" type="text" autocomplete="off" bind:value={toText} oninput={() => typed("to")} onkeydown={(e) => keydown("to", e)} onblur={() => typed("to", true)} />
          {:else}
            <input id="write-cc" type="text" autocomplete="off" bind:value={ccText} oninput={() => typed("cc")} onkeydown={(e) => keydown("cc", e)} onblur={() => typed("cc", true)} />
          {/if}
          {#if field === "to" && !copy}<button type="button" class="end" onclick={() => (copy = true)}>Add copy</button>{/if}
        </div>
      {/if}
    {/each}
    {#if reply}
      <p class="subject re">{reply.subject}</p>
    {:else}
      <input class="subject" type="text" placeholder="Subject" aria-label="Subject" bind:value={subject} />
    {/if}
    <textarea class="body" aria-label="Mail text" bind:value={body}></textarea>

    {#if flags.length > 0}
      <ul class="flags">{#each flags as f (f)}<li>{FLAGS[f] ?? f}</li>{/each}</ul>
    {/if}
    {#if unknown}
      <p role="alert">Iris does not know if it took the mail. Press Send again: Iris sends it only once.</p>
    {:else if problem}
      <p role="alert">{problem}</p>
    {/if}
    <div class="foot">
      <button type="submit" class="primary" disabled={!ready}>{busy ? "Sending" : "Send"}</button>
      <p class="hint">It waits 60 seconds in the Outbox, so you can still cancel it.</p>
      <button type="button" class="quiet" onclick={discard}>Discard</button>
    </div>
  </form>
{/if}

<style>
  .title {
    font-size: var(--font-size-2);
    font-weight: 500;
    margin: var(--space-4) 0 var(--space-2);
  }
  .parent {
    display: flex;
    gap: var(--space-3);
    align-items: flex-start;
    padding: var(--space-3) var(--space-4);
    border-left: 2px solid var(--color-border);
    margin: var(--space-4) 0 var(--space-6);
    font-size: var(--font-size--1);
  }
  .parent p {
    margin: 0;
  }
  .from {
    font-weight: 500;
  }
  .text {
    color: var(--color-text-muted);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .mark {
    flex: none;
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border-radius: var(--radius-full);
    background: var(--color-background-surface);
    color: var(--color-text-muted);
    font-weight: 600;
  }
  .faint {
    color: var(--color-text-faint);
  }
  .muted {
    color: var(--color-text-muted);
    max-width: 52ch;
    margin: 0;
  }
  .small {
    font-size: var(--font-size--1);
    margin: 0;
  }
  .line {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-3);
    padding: var(--space-3) 0;
    border-bottom: 1px solid var(--color-border-subtle);
  }
  .k {
    width: 48px;
    color: var(--color-text-faint);
    font-size: var(--font-size--1);
  }
  .line input,
  .line select {
    flex: 1;
    min-width: 120px;
    font: inherit;
    color: var(--color-text);
    background: none;
    border: 0;
    padding: var(--space-1) 0;
  }
  .line input:focus,
  .subject:focus,
  .body:focus {
    outline: none;
  }
  .end {
    margin-left: auto;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    background: var(--color-background-surface);
    border-radius: var(--radius-full);
    padding: 2px var(--space-1) 2px var(--space-3);
    font-size: var(--font-size--1);
    overflow-wrap: anywhere;
  }
  .chip.bad {
    box-shadow: inset 0 0 0 1px var(--color-error);
  }
  .chip button {
    font: inherit;
    color: var(--color-text-faint);
    background: none;
    border: 0;
    padding: 0 var(--space-1);
    cursor: pointer;
  }
  .subject {
    display: block;
    width: 100%;
    box-sizing: border-box;
    font: inherit;
    font-size: var(--font-size-1);
    font-weight: 500;
    color: var(--color-text);
    background: none;
    border: 0;
    padding: var(--space-4) 0 var(--space-2);
    margin: 0;
  }
  .subject.re {
    font-size: var(--font-size-0);
    font-weight: 400;
    color: var(--color-text-muted);
  }
  .body {
    display: block;
    width: 100%;
    box-sizing: border-box;
    min-height: 260px;
    field-sizing: content;
    resize: vertical;
    font: inherit;
    line-height: 1.7;
    color: var(--color-text);
    background: none;
    border: 0;
    padding: var(--space-4) 0;
  }
  .flags {
    list-style: none;
    margin: 0 0 var(--space-3);
    padding: 0;
    display: grid;
    gap: var(--space-1);
    font-size: var(--font-size--1);
  }
  .flags li {
    border-left: 4px solid var(--color-warning);
    padding-left: var(--space-3);
  }
  .foot,
  .row {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-4);
    padding-top: var(--space-4);
  }
  .hint {
    flex: 1;
    min-width: 180px;
    margin: 0;
    color: var(--color-text-faint);
    font-size: var(--font-size--1);
  }
  button {
    font: inherit;
    cursor: pointer;
  }
  button:disabled {
    cursor: default;
    opacity: 0.5;
  }
  .primary {
    background: var(--color-primary);
    color: var(--color-on-primary);
    border: 0;
    border-radius: var(--radius-full);
    padding: var(--space-2) var(--space-6);
    font-weight: 600;
  }
  .quiet,
  .end {
    background: none;
    border: 0;
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
    padding: var(--space-2);
  }
  .pill {
    color: var(--color-text);
    background: var(--color-background-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-full);
    padding: var(--space-2) var(--space-4);
    text-decoration: none;
  }
  .done {
    display: grid;
    gap: var(--space-4);
    justify-items: start;
    padding: var(--space-12) 0;
  }
  .done h1 {
    font-size: var(--font-size-2);
    font-weight: 500;
    margin: 0;
    overflow-wrap: anywhere;
  }
  .ring {
    width: 48px;
    height: 48px;
    border-radius: var(--radius-full);
    border: 2px solid var(--color-primary);
    display: grid;
    place-items: center;
    color: var(--color-primary);
    font-size: var(--font-size-1);
  }
  .ring.warn {
    border-color: var(--color-warning);
    color: var(--color-text);
  }
  .ring.stop {
    border-color: var(--color-border);
    color: var(--color-text-muted);
  }
  .bar {
    display: block;
    width: 240px;
    max-width: 100%;
    height: 4px;
    border-radius: var(--radius-full);
    background: var(--color-border-subtle);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--color-primary);
    transition: width var(--transition-normal);
  }
  .off {
    margin: 0;
    border-left: 4px solid var(--color-warning);
    padding-left: var(--space-3);
  }
  @media (prefers-reduced-motion: reduce) {
    .bar span {
      transition: none;
    }
  }
</style>
