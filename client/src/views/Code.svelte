<!--
  A login code that is still good: large digits, one button to copy, and the
  time left. At the moment it expires, the code leaves the screen (spec 1 section 6.6).
  With `ondone`, a quiet Dismiss holds the verb for the Undo time, as on a brief
  line (issue 527), then calls `ondone`. The brief sends each hold when the
  person leaves it.
-->
<script lang="ts">
  import { post, type Item } from "../api";
  import { hold, HOLD } from "../act";
  import { watch } from "../clock";
  import { copyText, liveCode, party, timeLeft } from "../format";
  import { href } from "../route";
  import AccountPill from "./AccountPill.svelte";

  let {
    item,
    now: start = new Date(),
    ondone,
    delay = HOLD,
  }: { item: Item; now?: Date; ondone?: (id: string) => void; delay?: number } = $props();
  // svelte-ignore state_referenced_locally
  let now = $state(start);
  let copied = $state<"" | "done" | "failed">("");

  $effect(() => (item.expires ? watch(item.expires, (d) => (now = d)) : undefined));

  type Phase = { at: "idle" } | { at: "held" } | { at: "sending" } | { at: "failed"; why: string; known: boolean };
  let phase: Phase = $state({ at: "idle" });
  let pending: ReturnType<typeof hold> | undefined;

  function dismiss() {
    phase = { at: "held" };
    pending = hold(send, delay);
  }

  function undo() {
    pending?.cancel();
    pending = undefined;
    phase = { at: "idle" };
  }

  async function send() {
    pending = undefined;
    phase = { at: "sending" };
    try {
      await post(`items/${encodeURIComponent(item.id)}/dismiss`, {});
      ondone?.(item.id);
    } catch (error) {
      const status = (error as { status?: number }).status ?? 0;
      // A 4xx answer wrote nothing. With no answer, or a 5xx, Iris cannot say.
      const known = status >= 400 && status < 500;
      phase = { at: "failed", why: (error as Error).message, known };
      // A 409: the code changed (for example, it expired) after the brief loaded it. The brief loads again.
      if (status === 409) ondone?.(item.id);
    }
  }

  async function copy() {
    copied = (await copyText(item.code ?? "")) ? "done" : "failed";
    setTimeout(() => (copied = ""), 4000);
  }
</script>

<!-- The held or failed Dismiss shows also after the expiry, so its Undo or its result does not go. -->
{#if phase.at !== "idle"}
  <div class="code said" class:error={phase.at === "failed"} role="status">
    {#if phase.at === "failed"}
      <span class="why">{phase.known ? `Iris did not record it: ${phase.why}` : "Iris does not know if it recorded the change. Reload the page to see."}</span>
      {#if phase.known}<button class="undo" type="button" onclick={() => (phase = { at: "idle" })}>Close</button>{/if}
    {:else}
      <span class="struck">{party(item.issuer)} code</span>
      <span class="muted">Dismissed. It leaves your brief.</span>
      {#if phase.at === "held"}
        <button class="undo" type="button" onclick={undo}>Undo</button>
        <span class="bar" style:animation-duration={`${delay}ms`} aria-hidden="true"></span>
      {/if}
    {/if}
  </div>
{:else if liveCode(item, now)}
  <div class="code">
    <p class="digits" aria-label="Login code">{item.code}</p>
    <div class="about">
      <a href={href.item(item.id)}>{party(item.issuer)}</a>
      <span>Ends {timeLeft(item.expires, now)}</span>
    </div>
    <AccountPill {item} />
    <button class="copy" type="button" onclick={copy}>{copied === "done" ? "Copied" : "Copy"}</button>
    {#if ondone}<button class="away" type="button" onclick={dismiss} aria-label={`Dismiss: ${party(item.issuer)} code`}>Dismiss</button>{/if}
    {#if copied === "failed"}<p class="failed" role="status">The browser did not allow the copy. Select the code and copy it.</p>{/if}
  </div>
{/if}

<style>
  .code {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2) var(--space-6);
    padding: var(--space-4) var(--space-6);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    background: var(--color-background-elevated);
  }
  .failed {
    flex-basis: 100%;
    margin: 0;
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  .digits {
    user-select: all;
    margin: 0;
    font-family: var(--font-family-mono);
    font-size: var(--font-size-3);
    letter-spacing: 0.16em;
    font-variant-numeric: tabular-nums;
  }
  .about {
    flex: 1 1 160px;
    min-width: 0;
    display: grid;
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  .about a {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-text);
    text-decoration: none;
    font-weight: 500;
  }
  .about a:hover {
    text-decoration: underline;
  }
  @media (max-width: 600px) {
    .code {
      padding: var(--space-4);
    }
    .digits {
      flex-basis: 100%;
    }
  }
  .copy {
    font: inherit;
    font-weight: 600;
    color: var(--color-on-primary);
    background: var(--color-primary);
    border: 0;
    border-radius: var(--radius-md);
    padding: var(--space-2) var(--space-4);
    min-width: calc(var(--space-24) - var(--space-4));
    cursor: pointer;
  }
  .copy:focus-visible {
    outline: 2px solid var(--color-text);
    outline-offset: 2px;
  }
  /* The quiet verb, as on a brief line, but always shown: a code card has no hover. */
  .away {
    padding: var(--space-1) var(--space-2);
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-text-muted);
    font: inherit;
    font-size: var(--font-size--1);
    cursor: pointer;
    transition: color var(--transition-fast);
  }
  .away:hover,
  .away:focus-visible {
    color: var(--color-text);
    background: var(--color-background-surface);
  }
  .away:focus-visible {
    outline: 2px solid var(--color-primary);
  }
  @media (max-width: 600px), (pointer: coarse) {
    .away,
    .undo {
      min-height: var(--size-touch);
    }
  }
  .said {
    position: relative;
    overflow: hidden;
    gap: var(--space-2) var(--space-3);
  }
  .said.error {
    border-left: 4px solid var(--color-error);
  }
  .struck {
    min-width: 0;
    color: var(--color-text-muted);
    text-decoration: line-through;
  }
  .muted {
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  .why {
    flex: 1;
    color: var(--color-text);
  }
  .undo {
    margin-left: auto;
    padding: var(--space-1) var(--space-2);
    border: 0;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--color-primary);
    font: inherit;
    font-weight: 600;
    cursor: pointer;
  }
  .undo:focus-visible {
    outline: 2px solid var(--color-primary);
  }
  /* The time left to undo: a primary line that shrinks. */
  .bar {
    position: absolute;
    left: 0;
    bottom: 0;
    height: 2px;
    width: 100%;
    background: var(--color-primary);
    transform-origin: left;
    animation: shrink linear forwards;
  }
  @keyframes shrink {
    to {
      transform: scaleX(0);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .bar {
      animation: none;
    }
  }
</style>
