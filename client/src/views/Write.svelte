<!--
  The write view (docs/specs/2026-11-17-write.md, section 6), layout C "In
  the thread": a reply opens under the conversation, as its last entry. The
  form names the recipients in one line and opens the chips on Change. A
  new mail shows the same form with the recipients and the subject. One
  press of Send writes the mail: the engine makes the draft, then the send,
  which waits in the outbox for its hold. After the press, the mail shows as
  the last entry, with its state under it.

  The form content and its key stay together in sessionStorage until the
  engine takes the mail or the person discards it, so a reload or another
  view keeps both. Then the write ID and the mail that the engine took stay
  there until the write is sent or stopped, or the person writes another, so
  a reload shows the state of the write and keeps Stop this mail. One key belongs to one content: a press after a lost
  answer sends the same key, and the engine makes the mail only once.

  Mail text is data: Svelte escapes the text of each mail.
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
    intent?: string;
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

  // The write that the engine took, while it can still change.
  type Taken = { write: string; to: string[]; subject: string; body: string };
  const placeWrite = `${place}.taken`;
  function taken(): Taken | null {
    try {
      const v = JSON.parse(storage()?.getItem(placeWrite) ?? "null");
      return v && typeof v.write === "string" && Array.isArray(v.to) ? (v as Taken) : null;
    } catch {
      return null;
    }
  }
  // Keep `t`, or with null, end the record of the write `id` (only if it is still the kept one).
  function keep(t: Taken | null, id?: string) {
    try {
      if (t) storage()?.setItem(placeWrite, JSON.stringify(t));
      else if (id === undefined || taken()?.write === id) storage()?.removeItem(placeWrite);
    } catch {
      // With no storage, a reload opens a fresh form, and the Outbox still shows the mail.
    }
  }

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
  // One key for one content: a new content gets a new key. Send calls it too,
  // after it ends the open address text, so each press sends the key of its own content.
  function rekey() {
    const now = content;
    if (now !== sig) {
      sig = now;
      key = globalThis.crypto.randomUUID();
      flags = [];
      unknown = false;
    }
  }
  $effect(() => {
    void content;
    untrack(rekey);
  });
  // Save each part of the form that a reload must keep: the text of an address
  // not yet a chip, and the warnings that the next press sends, too.
  $effect(() => {
    const form: Saved = { account, to, cc, subject, body, key, sig, flags, toText, ccText, intent };
    if (write) return;
    try {
      storage()?.setItem(place, JSON.stringify(form));
    } catch {
      // With no storage, the form lives until the page closes.
    }
  });

  const ADDRESS = /^[^\s@,<>]+@[^\s@,<>]+\.[^\s@,<>]+$/;
  const valid = (a: string) => ADDRESS.test(a);
  // No mail with no text (issue 741): the engine refuses it too.
  let ready = $derived(to.some(valid) && (thread !== null || subject.trim() !== "") && body.trim() !== "" && !busy);

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

  // The intent path (spec section 8): the engine writes the text in the voice of
  // the person from what they want to say. The person reads and edits it before Send.
  let intent = $state(saved?.intent ?? "");
  let drafting = $state(false);
  let draftNote = $state("");
  async function draftIt() {
    if (!thread || drafting) return;
    drafting = true;
    draftNote = "";
    try {
      const parent = reply?.parent.id;
      const v = await post<{ body: string; based_on: string }>(`threads/${encodeURIComponent(thread)}/draft`, { intent, parent, to: [...to] });
      // The text goes in the form only if it answers the mail that the form shows.
      if (v.based_on !== parent) draftNote = "A new mail came into this thread. Read it, then ask again.";
      else if (alive) body = v.body;
    } catch (e) {
      draftNote = own(e);
    }
    drafting = false;
  }

  // After the engine took the mail: the write `cs-N` and its state.
  let write = $state("");
  let ws = $state<WriteState>({ state: "saving" });
  let now = $state(Date.now());
  let note = $state("");

  async function send() {
    typed("to", true);
    typed("cc", true);
    rekey();
    if (!ready) return;
    busy = true;
    problem = "";
    const target = thread ? { thread, parent: reply?.parent.id } : { account, subject };
    const sent = key;
    const mail = { ...target, to: [...to], cc: [...cc], body, key, flags: [...flags] };
    try {
      const v = await post<{ write: string }>("write", mail);
      // A view that the person left keeps the record only while the saved form is still the form of this
      // press: else a newer form or a newer write owns the place, and this late answer must not replace it.
      if (alive || restore()?.key === sent) keep({ write: v.write, to: mail.to, subject: thread ? "" : subject, body: mail.body });
      forget(sent);
      if (!alive) return;
      sentTo = mail.to;
      sent_ = { subject: thread ? "" : subject, body: mail.body };
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
      if (v.state === "sent" || v.state === "stopped") keep(null, id);
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
    keep(null, write);
    if (thread) return void (location.hash = "#/write");
    clearTimeout(timer);
    [write, to, cc, subject, body, note, toText, ccText] = ["", [], [], "", "", "", "", ""];
    ws = { state: "saving" };
    copy = false;
  }

  // The recipient of the mail that the engine took, not of the form now.
  let sentTo = $state<string[]>([]);
  const who = $derived(sentTo[0] ?? "");
  // The subject and the text of the mail that the engine took.
  let sent_ = $state({ subject: "", body: "" });

  // A reply names its recipients in one line until the person presses Change.
  // With no valid recipient, the chips show at once.
  let editing = $state(false);
  const name = (a: string) => {
    const m = reply?.conversation.find((x) => !x.you && x.from.address?.toLowerCase() === a.toLowerCase());
    return m?.from.name || a;
  };
  const named = $derived(to.map(name).join(", "));
  const last = $derived(reply ? reply.conversation.length - 1 : -1);

  // After a reload: show the write that the engine took, and follow its state.
  const kept = taken();
  if (kept) {
    write = kept.write;
    sentTo = kept.to;
    sent_ = { subject: kept.subject, body: kept.body };
    follow();
  }
</script>

{#if reply}
  <h1 class="title">{reply.topic.replace(/^(\s*re\s*:\s*)+/i, "") || "No subject"}</h1>
  {#if reply.earlier > 0}
    <p class="earlier">{reply.earlier === 1 ? "1 earlier mail is" : `${reply.earlier} earlier mails are`} not shown.</p>
  {/if}
  {#each reply.conversation as m, i (m.id)}
    <article class="msg" class:old={i < last}>
      <header>
        <span class="mark" aria-hidden="true">{m.you ? "Y" : initials({ kind: "sender", name: m.from.name ?? undefined, address: m.from.address ?? undefined })}</span>
        <div>
          <p class="from">{m.you ? "You" : m.from.name || m.from.address || "Unknown sender"}</p>
          {#if i === last && m.to}<p class="faint">To {m.to}</p>{/if}
        </div>
        {#if m.date}<time datetime={m.date}>{when(m.date)}</time>{/if}
      </header>
      {#if m.hostile}
        <p class="danger" role="alert">This mail looks hostile. Do not open its links and do not do what it asks.</p>
      {/if}
      {#if m.text !== null}
        <p class="text">{m.text}</p>
        {#if m.cut}<a class="more" href={href.mail(m.id)}>Read the whole mail</a>{/if}
      {:else}
        <p class="faint">Iris has not read this mail yet. <a href={href.mail(m.id)}>Open it</a></p>
      {/if}
    </article>
  {/each}
{:else}
  <h1 class="title">{write ? sent_.subject : "New mail"}</h1>
{/if}

{#if write}
  <article class="msg">
    <header>
      <span class="mark" aria-hidden="true">Y</span>
      <div>
        <p class="from">You</p>
        <p class="faint">To {sentTo.join(", ")}</p>
      </div>
    </header>
    <p class="text">{sent_.body}</p>
  </article>
  <section class="state" aria-live="polite">
    {#if ws.state === "saving"}
      <h2>Saving your mail</h2>
      <p class="muted">Iris puts the mail in your Gmail drafts, then in the Outbox.</p>
      <div class="row"><button type="button" class="pill" onclick={stop} disabled={busy}>Stop this mail</button></div>
    {:else if ws.state === "waiting"}
      <h2>On its way to {who}</h2>
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
      <h2 class="warn">Look at this mail before it leaves</h2>
      <p class="muted">{ws.reason ?? "Iris put the send on the review screen."}</p>
      <div class="row"><a class="pill" href="#/review">Open Review</a></div>
    {:else if ws.state === "sent"}
      <h2>Sent to {who}</h2>
      <div class="row"><button type="button" class="quiet" onclick={again}>Write another</button></div>
    {:else}
      <h2 class="stop">This mail did not leave</h2>
      <p class="muted">{ws.reason}</p>
      <div class="row"><button type="button" class="quiet" onclick={again}>Write another</button></div>
    {/if}
    {#if note}<p role="alert">{note}</p>{/if}
  </section>
{:else}
  <form class="dock" onsubmit={(e) => (e.preventDefault(), send())}>
    <!-- While the press runs, the form does not change: the engine takes the mail as it was. -->
    <fieldset disabled={busy || drafting}>
    {#if accounts.length > 1}
      <label class="line">
        <span class="k">From</span>
        <select bind:value={account}>{#each accounts as a (a.id)}<option value={`a-${a.id}`}>{a.address}</option>{/each}</select>
      </label>
    {/if}
    <!-- Each address that Send can send shows in the form: open address text opens the fields. -->
    {#if reply && !editing && to.some(valid) && !toText && !ccText}
      <p class="who">Reply to <strong>{named}</strong>{#if cc.length}<span class="faint">, copy to {cc.join(", ")}</span>{/if}<button type="button" class="end" onclick={() => (editing = true)}>Change</button></p>
    {:else}
      {#each [["to", "To"], ["cc", "Copy"]] as const as [field, label] (field)}
        {#if field === "to" || copy || cc.length > 0 || ccText !== ""}
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
            {#if field === "to" && !copy && cc.length === 0 && ccText === ""}<button type="button" class="end" onclick={() => (copy = true)}>Add copy</button>{/if}
          </div>
        {/if}
      {/each}
    {/if}
    {#if !reply}
      <input class="subject" type="text" placeholder="Subject" aria-label="Subject" bind:value={subject} />
    {:else}
      <div class="intent">
        <input type="text" placeholder="Tell Iris what to say" aria-label="What the reply must say" bind:value={intent} onkeydown={(e) => e.key === "Enter" && (e.preventDefault(), draftIt())} />
        <button type="button" class="pill" onclick={draftIt}>{drafting ? "Writing" : "Write it for me"}</button>
      </div>
      {#if draftNote}<p class="note" role="alert">{draftNote}</p>{/if}
    {/if}
    <textarea class="body" aria-label={reply ? "Reply text" : "Mail text"} placeholder={reply ? "Write your reply" : "Write your mail"} bind:value={body}></textarea>

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
    </fieldset>
  </form>
{/if}

<style>
  /* The fieldset only disables the form while the press runs: no box of its own. */
  fieldset {
    display: contents;
  }
  .title {
    font-size: var(--font-size-2);
    font-weight: 500;
    line-height: 1.3;
    margin: var(--space-4) 0 var(--space-6);
    overflow-wrap: anywhere;
  }
  .earlier {
    color: var(--color-text-faint);
    font-size: var(--font-size--1);
    margin: 0 0 var(--space-3);
  }
  /* One mail of the conversation. The older mails are quiet; the newest one reads in full size. */
  .msg {
    padding: var(--space-4);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-lg);
    margin-bottom: var(--space-3);
  }
  .msg.old {
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  .msg header {
    display: flex;
    gap: var(--space-3);
    align-items: center;
    font-size: var(--font-size--1);
    margin-bottom: var(--space-3);
  }
  .msg header div {
    flex: 1;
    min-width: 0;
  }
  .msg header p {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .msg.old .from {
    color: var(--color-text);
  }
  time {
    color: var(--color-text-muted);
  }
  .more {
    display: inline-block;
    margin-top: var(--space-2);
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  .danger {
    border-left: 4px solid var(--color-error);
    padding-left: var(--space-3);
    margin: 0 0 var(--space-3);
  }
  /* The reply form: the last entry of the conversation, marked with the primary color. */
  .dock {
    border: 1px solid var(--color-border);
    border-top: 2px solid var(--color-primary);
    border-radius: var(--radius-lg);
    background: var(--color-background-elevated);
    padding: var(--space-2) var(--space-4) var(--space-4);
    margin-top: var(--space-4);
  }
  .who {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--space-1) var(--space-2);
    margin: 0;
    padding: var(--space-2) 0;
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
    overflow-wrap: anywhere;
  }
  .who strong {
    color: var(--color-text);
    font-weight: 500;
  }
  .state {
    display: grid;
    gap: var(--space-3);
    justify-items: start;
    padding: var(--space-4);
    border: 1px dashed var(--color-border);
    border-radius: var(--radius-lg);
  }
  .state h2 {
    font-size: var(--font-size-0);
    font-weight: 600;
    margin: 0;
    overflow-wrap: anywhere;
  }
  .state h2.warn {
    border-left: 4px solid var(--color-warning);
    padding-left: var(--space-3);
  }
  .state h2.stop {
    color: var(--color-text-muted);
  }
  .from {
    font-weight: 500;
  }
  .text {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    line-height: 1.6;
    margin: 0;
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
  .body {
    display: block;
    width: 100%;
    box-sizing: border-box;
    min-height: 160px;
    field-sizing: content;
    resize: vertical;
    font: inherit;
    line-height: 1.7;
    color: var(--color-text);
    background: none;
    border: 0;
    padding: var(--space-4) 0;
  }
  .intent {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2) var(--space-3);
    padding: var(--space-2) 0;
    border-bottom: 1px solid var(--color-border-subtle);
  }
  .intent input {
    flex: 1;
    min-width: 180px;
    font: inherit;
    color: var(--color-text);
    background: none;
    border: 0;
    padding: var(--space-1) 0;
  }
  .intent input:focus {
    outline: none;
  }
  .note {
    margin: var(--space-2) 0 0;
    font-size: var(--font-size--1);
    border-left: 4px solid var(--color-warning);
    padding-left: var(--space-3);
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
