<!--
  What the person does with an open or deferred commitment: Done, Later or
  Decline. A click waits a few seconds with Undo before Iris records it, so a
  wrong click costs nothing. Iris records the change through a changeset and
  the ledger (spec 2 section 4).
-->
<script lang="ts">
  import type { Snippet } from "svelte";
  import { get, post, type Item } from "../api";
  import { day, first, firstName, hold, HOLD, nextWeek, said, started, type Verb } from "../act";

  // `delay`: the time for Undo, shorter in tests. `clock`: the time now.
  let {
    item,
    onchange,
    delay = HOLD,
    clock = () => new Date(),
    more,
  }: { item: Item; onchange: (item: Item) => void; delay?: number; clock?: () => Date; more?: Snippet } = $props();

  type Phase =
    | { at: "idle" }
    | { at: "held"; verb: Verb; date?: string }
    | { at: "sending"; verb: Verb; date?: string }
    | { at: "done"; verb: Verb; date?: string }
    | { at: "failed"; verb: Verb; date?: string; why: string; known: boolean };
  let phase: Phase = $state({ at: "idle" });
  let later = $state(false);
  let picked = $state("");
  let pending: ReturnType<typeof hold> | undefined;

  let open = $derived(item.form === "commitment" && (item.status === "open" || item.status === "deferred"));
  // The days of Later come from the time when Later opens, and a click checks them again.
  let now = $state(new Date(0));
  let soon = $derived(first(now));
  let week = $derived(nextWeek(now));

  function openLater() {
    now = clock();
    later = !later;
  }

  function defer(date: string | null) {
    // A page kept open past midnight: a day that started in UTC would open the item again at once.
    if (!date || started(date, clock())) {
      now = clock();
      picked = "";
      return;
    }
    choose("defer", date);
  }

  function choose(verb: Verb, date?: string) {
    later = false;
    phase = { at: "held", verb, date };
    pending = hold(() => send(verb, date), delay);
  }

  function undo() {
    pending?.cancel();
    pending = undefined;
    phase = { at: "idle" };
  }

  async function send(verb: Verb, date?: string) {
    pending = undefined;
    // The day can start in UTC during the hold: then send nothing, and offer the days again.
    if (date && started(date, clock())) {
      now = clock();
      later = true;
      phase = { at: "idle" };
      return;
    }
    phase = { at: "sending", verb, date };
    try {
      const next = await post<Item>(`items/${encodeURIComponent(item.id)}/${verb}`, date ? { date } : {});
      phase = { at: "done", verb, date };
      onchange(next);
    } catch (error) {
      const status = (error as { status?: number }).status ?? 0;
      // A 4xx answer wrote nothing. With no answer, or a 5xx, Iris cannot say.
      const known = status >= 400 && status < 500;
      phase = { at: "failed", verb, date, why: (error as Error).message, known };
      // A 409: the item changed after the page loaded it (for example, understanding closed it). Show it as it is now.
      if (status === 409) {
        try {
          onchange(await get<Item>(`items/${encodeURIComponent(item.id)}`));
        } catch {
          // The refusal stays on the page. A reload shows the item.
        }
      }
    }
  }

  // If the person leaves during the hold (another view, a reload or a closed
  // tab), the click stands: send it now. The POST uses keepalive.
  $effect(() => {
    const leave = () => pending?.flush();
    window.addEventListener("pagehide", leave);
    return () => {
      window.removeEventListener("pagehide", leave);
      leave();
    };
  });
</script>

{#if phase.at !== "idle"}
  <div class="said" role="status">
    {#if phase.at === "failed"}
      <p class="error">
        {phase.known ? `Iris did not record it: ${phase.why}` : "Iris does not know if it recorded the change. Reload the page to see."}
        {#if phase.known}<button class="quiet" type="button" onclick={() => (phase = { at: "idle" })}>Close</button>{/if}
      </p>
    {:else}
      <p>
        <svg class="tick" viewBox="0 0 20 20" width="20" height="20" aria-hidden="true">
          <circle cx="10" cy="10" r="9" />
          <path d="M6 10.5l2.75 2.75L14 8" />
        </svg>
        <span>{said(phase.verb, phase.date)}</span>
        {#if phase.at === "held"}
          <span class="muted">It leaves your brief.</span>
          <button class="quiet" type="button" onclick={undo}>Undo</button>
        {:else if phase.at === "done"}
          <a class="muted" href="#/">Back to the brief</a>
        {/if}
      </p>
      {#if phase.at === "held"}<span class="bar" style:animation-duration={`${delay}ms`} aria-hidden="true"></span>{/if}
    {/if}
  </div>
{:else if open}
  <div class="act">
    <div class="row">
      <button class="done" type="button" onclick={() => choose("close")}>
        <svg viewBox="0 0 20 20" width="18" height="18" aria-hidden="true"><path d="M5 10.5l3.25 3.25L15 7" /></svg>
        Done
      </button>
      <button class="pill" type="button" aria-expanded={later} onclick={openLater}>Later</button>
      <button class="quiet" type="button" onclick={() => choose("decline")}>Decline</button>
      {@render more?.()}
    </div>
    {#if later}
      <div class="when">
        <button class="chip" type="button" onclick={() => defer(soon)}>{firstName(now)} <span>{day(soon)}</span></button>
        {#if week && week !== soon}<button class="chip" type="button" onclick={() => defer(week)}>Next week <span>{day(week)}</span></button>{/if}
        <label class="chip">
          Pick a date
          <input type="date" min={soon} bind:value={picked} onchange={() => defer(picked)} />
        </label>
      </div>
    {/if}
  </div>
{/if}

<style>
  .act,
  .said {
    margin: 0 0 var(--space-8);
  }
  .row,
  .when {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }
  .when {
    margin-top: var(--space-3);
  }
  button,
  .chip {
    font: inherit;
    cursor: pointer;
    min-height: calc(var(--space-8) + var(--space-2));
    border-radius: var(--radius-full);
    transition: background var(--transition-fast), border-color var(--transition-fast), color var(--transition-fast);
  }
  .done {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    padding: 0 var(--space-6) 0 var(--space-4);
    border: 0;
    background: var(--color-primary);
    color: var(--color-on-primary);
    font-weight: 600;
  }
  .done svg {
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .pill,
  .chip {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    padding: 0 var(--space-4);
    border: 1px solid var(--color-border);
    background: transparent;
    color: var(--color-text);
  }
  .pill:hover,
  .pill[aria-expanded="true"],
  .chip:hover,
  .chip:focus-within {
    border-color: var(--color-primary);
  }
  .chip span {
    color: var(--color-text-muted);
  }
  .chip input {
    font: inherit;
    color: var(--color-text-muted);
    background: transparent;
    border: 0;
    color-scheme: inherit;
  }
  .quiet {
    padding: 0 var(--space-3);
    border: 0;
    background: transparent;
    color: var(--color-text-muted);
  }
  .quiet:hover {
    color: var(--color-text);
  }
  button:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }
  .said {
    position: relative;
    padding: var(--space-3) var(--space-4);
    border-radius: var(--radius-md);
    background: var(--color-background-elevated);
    overflow: hidden;
  }
  .said p {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
    margin: 0;
  }
  .tick {
    flex: none;
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
  .muted {
    color: var(--color-text-muted);
  }
  .said .quiet {
    min-height: 0;
    margin-left: auto;
    color: var(--color-primary);
    font-weight: 600;
  }
  .error {
    color: var(--color-text);
    border-left: 4px solid var(--color-error);
    padding-left: var(--space-3);
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
    button,
    .chip,
    .tick {
      transition: none;
      animation: none;
    }
    .bar {
      display: none;
    }
  }
</style>
