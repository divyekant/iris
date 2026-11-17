<!--
  One item on one line. The whole line opens the item. The envelope opens the
  source mail in one step. An unread dot marks a conversation that waits for you.
  A "Draft ready" pill marks an item with a reply that Iris prepared (card R3-09).
  With `ondone`, an open commitment starts with a round tick box: a click marks
  it done after the same Undo time as the item view, then calls `ondone`. The
  view that passes `ondone` calls `flushHolds` when the person leaves it.
-->
<script lang="ts">
  import type { Item } from "../api";
  import { due, initials, money, origin, other, party, timeLeft, when } from "../format";
  import { href } from "../route";
  import { post } from "../api";
  import { hold, HOLD } from "../act";

  // `note`: one more muted line, for example the reason of a rule that hides the item.
  // `ondone`: shows the tick box on an open commitment. `delay`: the time for Undo, shorter in tests.
  let {
    item,
    now = new Date(),
    twins = [],
    note = "",
    ondone,
    delay = HOLD,
  }: { item: Item; now?: Date; twins?: string[]; note?: string; ondone?: (id: string) => void; delay?: number } = $props();

  let box = $derived(
    !!ondone && item.form === "commitment" && (item.status === "open" || item.status === "deferred"),
  );
  type Phase = { at: "idle" } | { at: "held" } | { at: "sending" } | { at: "done" } | { at: "failed"; why: string; known: boolean };
  let phase: Phase = $state({ at: "idle" });
  let pending: ReturnType<typeof hold> | undefined;

  function tick() {
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
      await post(`items/${encodeURIComponent(item.id)}/close`, {});
      phase = { at: "done" };
      ondone?.(item.id);
    } catch (error) {
      const status = (error as { status?: number }).status ?? 0;
      // A 4xx answer wrote nothing. With no answer, or a 5xx, Iris cannot say.
      const known = status >= 400 && status < 500;
      phase = { at: "failed", why: (error as Error).message, known };
      // A 409: the item changed after the brief loaded it. The brief loads again.
      if (status === 409) ondone?.(item.id);
    }
  }

  // The hold outlives this line: a brief refresh can remove the row while Undo
  // time remains. The brief sends each hold when the person leaves it.

  let who = $derived(other(item));
  let date = $derived(item.form === "commitment" ? due(item, now) : null);
  let detail = $derived.by(() => {
    switch (item.form) {
      case "commitment":
        return [party(who), money(item.amount)].filter(Boolean).join(", ");
      case "conversation":
        return item.latest ? `${party(item.latest.from)}: ${item.latest.snippet}` : party(who);
      case "feed":
        return `${party(who)}, ${when(item.published)}`;
      case "signal": {
        const left = timeLeft(item.expires, now);
        return `${party(who)}, ${left ? `ends ${left}` : "ended"}`;
      }
      default:
        return party(who);
    }
  });
  let waits = $derived(item.form === "conversation" && (item.your_turn || (item.new ?? 0) > 0));
</script>

{#if phase.at !== "idle"}
  <li class="line said" class:error={phase.at === "failed"} role="status">
    {#if phase.at === "failed"}
      <span class="why">
        {phase.known ? `Iris did not record it: ${phase.why}` : "Iris does not know if it recorded the change. Reload the page to see."}
      </span>
      {#if phase.known}<button class="undo" type="button" onclick={() => (phase = { at: "idle" })}>Close</button>{/if}
    {:else}
      <svg class="tick" viewBox="0 0 20 20" width="20" height="20" aria-hidden="true">
        <circle cx="10" cy="10" r="9" />
        <path d="M6 10.5l2.75 2.75L14 8" />
      </svg>
      <span class="struck">{item.title}</span>
      <span class="muted">Done. It leaves your brief.</span>
      {#if phase.at === "held"}
        <button class="undo" type="button" onclick={undo}>Undo</button>
        <span class="bar" style:animation-duration={`${delay}ms`} aria-hidden="true"></span>
      {/if}
    {/if}
  </li>
{:else}
<li class="line">
  {#if box}
    <button class="box" type="button" aria-label={`Done: ${item.title}`} title="Done" onclick={tick}>
      <svg viewBox="0 0 20 20" width="20" height="20" aria-hidden="true">
        <circle cx="10" cy="10" r="9" />
        <path d="M6 10.5l2.75 2.75L14 8" />
      </svg>
    </button>
  {:else}
    <span class="mark" aria-hidden="true">{initials(who)}</span>
  {/if}
  <div class="body">
    <a class="title" href={href.item(item.id)}>{item.title}</a>
    <p class="detail">
      {detail}{#if date}<span class="due {date.tone}">, {date.text}</span>{/if}
    </p>
    {#if item.ready?.includes("draft")}
      <p class="prepared">
        <span class="ready" title="Iris wrote a reply in your voice">
          <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
            <path d="M10.5 2.5l3 3-7.5 7.5H3v-3z" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round" />
          </svg>
          Draft ready
        </span>
      </p>
    {/if}
    {#if note}<p class="note">{note}</p>{/if}
    {#if twins.length}
      <p class="twins">
        Maybe the same as {#each twins as id, i (id)}{i ? ", " : ""}<a href={href.item(id)}>{id}</a>{/each}
      </p>
    {/if}
  </div>
  {#if waits}<span class="dot" title={item.your_turn ? "Your turn" : `${item.new} new`}><span class="hidden">{item.your_turn ? "Your turn" : `${item.new} new`}</span></span>{/if}
  <a class="source" href={href.mail(origin(item))} aria-label="Open the source mail" title="Open the source mail">
    <svg viewBox="0 0 20 20" width="20" height="20" aria-hidden="true">
      <rect x="2.5" y="4.5" width="15" height="11" rx="2" fill="none" stroke="currentColor" stroke-width="1.5" />
      <path d="M3 5.5l7 5.5 7-5.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round" />
    </svg>
  </a>
</li>
{/if}

<style>
  .line {
    position: relative;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-2);
    margin: 0 calc(-1 * var(--space-2));
    border-radius: var(--radius-md);
    transition: background var(--transition-fast);
  }
  .line:hover {
    background: var(--color-background-surface);
  }
  .mark {
    flex: none;
    display: grid;
    place-items: center;
    width: var(--space-8);
    height: var(--space-8);
    border-radius: var(--radius-full);
    background: var(--color-background-surface);
    border: 1px solid var(--color-border-subtle);
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
    font-weight: 600;
  }
  .body {
    flex: 1;
    min-width: 0;
  }
  .title {
    color: var(--color-text);
    text-decoration: none;
    font-weight: 500;
  }
  /* The title link covers the line, so the whole line opens the item. */
  .title::after {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: var(--radius-md);
  }
  .title:focus-visible {
    outline: none;
  }
  .title:focus-visible::after {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }
  .detail {
    margin: 0;
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
  }
  .note,
  .twins {
    margin: var(--space-1) 0 0;
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  .twins a {
    position: relative;
    z-index: 1;
    color: var(--color-text-muted);
  }
  .due::after {
    content: "";
    display: inline-block;
    width: var(--space-2);
    height: var(--space-2);
    margin: 0 0 1px var(--space-2);
    border-radius: var(--radius-full);
    background: transparent;
  }
  .due.soon::after {
    background: var(--color-warning);
  }
  .due.late::after {
    background: var(--color-error);
  }
  /* What Iris prepared: quiet pills under the detail. They are no control, so a click opens the item. */
  .prepared {
    margin: var(--space-1) 0 0;
  }
  .ready {
    display: inline-flex;
    align-items: center;
    gap: var(--space-1);
    padding: 0 var(--space-2);
    border: 1px solid var(--color-primary);
    border-radius: var(--radius-full);
    color: var(--color-primary);
    font-size: var(--font-size--1);
    white-space: nowrap;
  }
  .dot {
    flex: none;
    width: var(--space-2);
    height: var(--space-2);
    border-radius: var(--radius-full);
    background: var(--color-unread);
  }
  .hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }
  .source {
    position: relative;
    z-index: 1;
    flex: none;
    display: grid;
    place-items: center;
    width: var(--space-8);
    height: var(--space-8);
    border-radius: var(--radius-md);
    color: var(--color-text-faint);
    transition: color var(--transition-fast), background var(--transition-fast);
  }
  .source:hover,
  .source:focus-visible {
    color: var(--color-text);
    background: var(--color-background-elevated);
    outline: none;
  }
  .source:focus-visible {
    box-shadow: 0 0 0 2px var(--color-primary);
  }
  /* The tick box: a ring that shows the tick on hover or focus. */
  .box {
    position: relative;
    z-index: 1;
    flex: none;
    display: grid;
    place-items: center;
    width: var(--space-8);
    height: var(--space-8);
    padding: 0;
    border: 0;
    border-radius: var(--radius-full);
    background: transparent;
    cursor: pointer;
  }
  .box circle {
    fill: none;
    /* A control edge needs a contrast of 3 or more (WCAG 1.4.11): the muted text color has it. */
    stroke: var(--color-text-muted);
    stroke-width: 1.5;
    transition: stroke var(--transition-fast);
  }
  .box path {
    fill: none;
    stroke: transparent;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
    transition: stroke var(--transition-fast);
  }
  .box:hover circle,
  .box:focus-visible circle,
  .box:hover path,
  .box:focus-visible path {
    stroke: var(--color-primary);
  }
  .box:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }
  .said {
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-3);
    background: var(--color-background-elevated);
    overflow: hidden;
  }
  .said:hover {
    background: var(--color-background-elevated);
  }
  .said.error {
    border-left: 4px solid var(--color-error);
  }
  .tick {
    flex: none;
    margin: 0 var(--space-2);
    animation: pop var(--transition-normal);
  }
  .tick circle {
    fill: var(--color-primary);
  }
  .tick path {
    fill: none;
    stroke: var(--color-on-primary);
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .struck {
    min-width: 0;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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
    animation-name: shrink;
    animation-timing-function: linear;
    animation-fill-mode: forwards;
  }
  @keyframes shrink {
    to {
      transform: scaleX(0);
    }
  }
  @keyframes pop {
    from {
      transform: scale(0.6);
      opacity: 0;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .line,
    .source,
    .box circle,
    .box path {
      transition: none;
    }
    .tick {
      animation: none;
    }
  }
</style>
