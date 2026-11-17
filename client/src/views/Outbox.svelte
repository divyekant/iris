<!--
  The outbox (spec docs/specs/2026-11-17-held-outbox.md, section 6): each
  send that a person approved and that waits for its hold or for the send
  switch. Each send shows its recipients, its subject, its warnings, the
  time left, and a Cancel button. A cancel works until the send starts.
  The view reads the switch. Only `iris sending on|off` changes it.
-->
<script lang="ts">
  import { onDestroy } from "svelte";
  import { loadOutbox, own, post, type Outbox, type OutboxSend } from "../api";
  import { poll } from "../clock";
  import { FLAGS } from "../review";

  let { first }: { first: Outbox } = $props();

  // svelte-ignore state_referenced_locally
  let box = $state(first);
  let now = $state(Date.now());
  let busy = $state("");
  // What each cancel did, as lines above the cards. The cards show only the
  // sends that the engine gives, so a new send of the same draft shows too.
  let notes = $state<string[]>([]);

  const tick = setInterval(() => (now = Date.now()), 1000);
  const stop = poll(loadOutbox, (v) => (box = v), 5000);
  onDestroy(() => {
    clearInterval(tick);
    stop();
  });


  /** The time left as "0:42" or "3:05", or null when the send can leave now. */
  function left(s: OutboxSend): string | null {
    if (!s.leaves) return null;
    const ms = Date.parse(s.leaves) - now;
    if (ms <= 0) return null;
    const sec = Math.ceil(ms / 1000);
    return `${Math.floor(sec / 60)}:${String(sec % 60).padStart(2, "0")}`;
  }

  /** The part of the hold that is gone, from 0 to 1, for the line of each card. */
  function gone(s: OutboxSend): number {
    if (!s.leaves) return 0;
    const end = Date.parse(s.leaves);
    const start = end - 60_000;
    return Math.min(1, Math.max(0, (now - start) / 60_000));
  }

  function when(s: OutboxSend): string {
    if (!box.sending.on) return "Waits until sending is on";
    const t = left(s);
    return t ? `Leaves in ${t}` : "Leaves now";
  }

  const to = (s: OutboxSend) => [...(s.to ?? []), ...(s.cc ?? [])];

  const name = (s: OutboxSend) => (s.readable && s.subject ? `"${s.subject}"` : `The send of ${s.draft}`);

  async function cancel(s: OutboxSend) {
    busy = s.draft;
    try {
      await post(`outbox/${encodeURIComponent(s.draft)}/cancel`, {});
      notes = [...notes, `${name(s)} is cancelled. It does not leave.`];
    } catch (e) {
      // A 409: the release claimed the send. The note stays after its card goes.
      const started = (e as { status?: number }).status === 409;
      notes = [...notes, started ? `${name(s)} started to send. It cannot be cancelled.` : `${name(s)}: ${own(e)}`];
    } finally {
      busy = "";
    }
    try {
      box = await loadOutbox();
    } catch {
      // The poll tries again.
    }
  }
</script>

<h1>Outbox</h1>
<p class="lead">Each mail that you approved waits here for one minute before it leaves. Cancel it here if you change your mind.</p>

{#if !box.sending.on}
  <p class="off" role="status"><strong>Sending is off. Sends wait here.</strong> To turn it on, run <code>iris sending on</code> at the terminal.</p>
{/if}

{#if notes.length > 0}
  <ul class="notes" role="status">{#each notes as note, i (i)}<li>{note}</li>{/each}</ul>
{/if}

{#if box.sends.length === 0}
  <p class="empty">No mail waits to leave.</p>
{:else}
  <ul class="cards">
    {#each box.sends as s (s.draft)}
      <li class="card">
        <p class="to">To {s.readable ? to(s).join(", ") : "recipients that Iris cannot read"}</p>
        <p class="subject">{s.readable ? s.subject || "(no subject)" : "Iris cannot read this draft"}</p>
        {#if s.flags.length > 0}
          <ul class="flags">{#each s.flags as f (f)}<li>{FLAGS[f] ?? f}</li>{/each}</ul>
        {/if}
        <div class="foot">
          <div class="time">
            <span class="left" class:paused={!box.sending.on}>{when(s)}</span>
            {#if box.sending.on}<span class="bar" aria-hidden="true"><span style:width="{gone(s) * 100}%"></span></span>{/if}
          </div>
          <button type="button" class="pill" onclick={() => cancel(s)} disabled={!!busy}>{busy === s.draft ? "Cancelling" : "Cancel"}</button>
        </div>
      </li>
    {/each}
  </ul>
{/if}

<style>
  h1 {
    font-size: var(--font-size-3);
    font-weight: 500;
    letter-spacing: -0.01em;
    margin: var(--space-4) 0 var(--space-2);
  }
  .lead,
  .empty {
    color: var(--color-text-muted);
    margin: 0 0 var(--space-8);
    max-width: 60ch;
  }
  .off {
    margin: 0 0 var(--space-6);
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--color-border);
    border-left: 4px solid var(--color-warning);
    border-radius: var(--radius-md);
    background: var(--color-background-surface);
    color: var(--color-text-muted);
  }
  .off strong {
    color: var(--color-text);
    font-weight: 600;
  }
  code {
    font-family: var(--font-family-mono);
    font-size: var(--font-size--1);
  }
  .notes {
    display: grid;
    gap: var(--space-1);
    margin: 0 0 var(--space-4);
    color: var(--color-text-muted);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  p {
    margin: 0;
  }
  .to {
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
    overflow-wrap: anywhere;
  }
  .subject {
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .left {
    font-variant-numeric: tabular-nums;
    color: var(--color-text);
    font-size: var(--font-size--1);
  }
  .left.paused {
    color: var(--color-text-faint);
  }
  button {
    font: inherit;
    cursor: pointer;
  }
  button:disabled {
    cursor: default;
    opacity: 0.6;
  }

  /* One card for each send, with the hold as a line. */
  .cards {
    display: grid;
    gap: var(--space-3);
  }
  .card {
    display: grid;
    gap: var(--space-1);
    padding: var(--space-4);
    background: var(--color-background-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
  }
  .card .subject {
    font-size: var(--font-size-1);
    font-weight: 500;
  }
  .flags {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1) var(--space-4);
    margin-top: var(--space-1);
  }
  .flags li {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  .flags li::before {
    content: "";
    width: 8px;
    height: 8px;
    border-radius: var(--radius-full);
    background: var(--color-warning);
    flex: none;
  }
  .foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    margin-top: var(--space-3);
  }
  .time {
    display: grid;
    gap: var(--space-1);
    flex: 1;
    max-width: 240px;
  }
  .bar {
    display: block;
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
  .pill {
    color: var(--color-text);
    background: var(--color-background-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-full);
    padding: var(--space-2) var(--space-4);
  }
  .pill:hover:not(:disabled) {
    border-color: var(--color-primary);
  }

</style>
