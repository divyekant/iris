<!--
  One Stop for all agent work (card R4-11, docs/specs/2026-11-17-stop.md).
  In the header: the Stop button. While Iris is stopped: a band above each
  view that says so, with Resume. A stop needs no confirmation: it changes
  no mail, and Resume undoes it.
-->
<script lang="ts">
  import { when } from "../format";

  let { place }: { place: "button" | "band" } = $props();

  /** The time of the stop: only the clock time on the same day. */
  function since(value: string): string {
    const at = new Date(value);
    return at.toDateString() === new Date().toDateString() ? at.toLocaleTimeString(undefined, { timeStyle: "short" }) : when(value);
  }
  const shared = stopState;
</script>

<script module lang="ts">
  import { get as read, post, own } from "../api";

  type Answer = { stopped: boolean; since: string | null; live?: number; busy?: boolean; resumed?: boolean };
  type Shared = { stopped: boolean; since: string | null; live: number; busy: boolean; known: boolean; busyAction: "" | "stop" | "resume"; note: string; error: string };
  /**
   * One state for the button and the band. Only an answer of the engine sets
   * it. `known` is false after a Stop or a Resume with no answer, until a read
   * of GET /api/stop gives the state again.
   */
  export const stopState: Shared = $state({ stopped: false, since: null, live: 0, busy: false, known: true, busyAction: "", note: "", error: "" });

  // The number of the newest request. Only the answer of the newest request
  // changes the state, so an older answer never replaces a newer one.
  let newest = 0;
  // The Stop or Resume that got no good answer. Until a read shows that it
  // took effect, the state stays not known: the action can still commit
  // after the read. A new action, or a read that shows it, clears it.
  let failedAction: "" | "stop" | "resume" = "";

  function show(n: number, a: Answer): boolean {
    if (n !== newest) return false;
    const confirmed = !failedAction || a.stopped === (failedAction === "stop");
    if (confirmed) failedAction = "";
    Object.assign(stopState, { stopped: a.stopped, since: a.since, live: a.stopped ? (a.live ?? 0) : 0, busy: a.stopped && !!a.busy, known: confirmed });
    if (confirmed) stopState.error = "";
    return true;
  }

  export async function reread() {
    const n = ++newest;
    try {
      show(n, await read<Answer>("stop"));
    } catch {
      // With no answer, the state is not known: the band makes no claim.
      // The access state shows the failure of the engine above each view.
      if (n === newest) stopState.known = false;
    }
  }

  /** The action to try again while the state is not known. */
  export function pending(): "" | "stop" | "resume" {
    return failedAction;
  }

  /** Read the state again: at start, at each new view and when the tab comes back. */
  export async function refresh() {
    // A Stop or a Resume in progress gives the newer state.
    if (stopState.busyAction) return;
    await reread();
  }

  /** Stop all agent work, or resume it. */
  export async function act(kind: "stop" | "resume") {
    const n = ++newest;
    failedAction = "";
    Object.assign(stopState, { busyAction: kind, error: "", note: "" });
    let failed = false;
    try {
      const a = await post<Answer>(kind, {});
      show(n, kind === "resume" ? { ...a, stopped: false, since: null } : a);
      // Only a real resume starts a new hold of each held send.
      if (kind === "resume" && n === newest && a.resumed) stopState.note = "Iris runs again. Each held send waits a full minute before it leaves.";
    } catch (e) {
      // The change can have happened with no answer: the state is not known
      // until the engine says it again.
      failed = true;
      failedAction = kind;
      Object.assign(stopState, {
        known: false,
        error: own(e) || (kind === "stop" ? "Iris did not answer the Stop. Try again, or run iris stop at the terminal." : "Iris did not answer the Resume. Try again."),
      });
    } finally {
      stopState.busyAction = "";
    }
    if (failed) await reread();
  }
</script>

{#if place === "button" && (!shared.stopped || !shared.known)}
  <button
    class="stop-all"
    type="button"
    disabled={shared.busyAction === "stop"}
    aria-label={shared.busyAction === "stop" ? "Stopping all agent work" : "Stop all agent work"}
    title="Stop all agent work. Each running agent ends. After Resume, run a job again to start it."
    onclick={() => act("stop")}
  >
    <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true"><rect x="2" y="2" width="12" height="12" rx="2" /></svg>
  </button>
{/if}

{#if place === "band"}
  {#if shared.error}<p class="band failed" role="alert">{shared.error}</p>{/if}
  {#if !shared.known}
    <section class="band" role="status" aria-label="Stop state not known">
      <p class="words">Iris does not know if agent work is stopped now.</p>
      <div class="actions">
        {#if shared.error && pending()}
          <button class="again" type="button" disabled={shared.busyAction !== ""} onclick={() => act(pending() || "stop")}>{pending() === "stop" ? "Stop again" : "Resume again"}</button>
        {/if}
        <button class="again" type="button" disabled={shared.busyAction !== ""} onclick={() => reread()}>Check again</button>
      </div>
    </section>
  {:else if shared.stopped}
    <section class="band" role="status" aria-label="Iris is stopped">
      <div class="words">
        <p><strong>Iris is stopped</strong>{#if shared.since}{" "}<span class="since">since {since(shared.since)}</span>{/if}.</p>
        {#if shared.live === 0 && !shared.busy}
          <p class="muted">No agent runs, no action applies and no mail leaves.</p>
        {:else}
          <p class="muted">The stop is not complete yet.</p>
        {/if}
        {#if shared.live > 0}
          <p class="muted">{shared.live === 1 ? "1 agent session still stops" : `${shared.live} agent sessions still stop`}. Press Stop again in a moment.</p>
        {/if}
        {#if shared.busy}
          <p class="muted">A Gmail change was in progress at the stop. <a href="#/history">History</a> shows its result.</p>
        {/if}
      </div>
      <div class="actions">
        {#if shared.live > 0 || shared.busy}
          <button
            class="again"
            type="button"
            disabled={shared.busyAction !== ""}
            onclick={() => act("stop")}>Stop again</button>
        {/if}
        <button
          class="resume"
          type="button"
          disabled={shared.busyAction !== ""}
          onclick={() => act("resume")}>{shared.busyAction === "resume" ? "Resuming" : "Resume"}</button>
      </div>
    </section>
  {:else if shared.note}
    <p class="note" role="status">{shared.note}</p>
  {/if}
{/if}

<style>
  .stop-all {
    margin-left: auto;
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: calc(var(--space-8) + var(--space-1));
    height: calc(var(--space-8) + var(--space-1));
    padding: 0;
    background: none;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-full);
    cursor: pointer;
    transition: border-color var(--transition-fast), background var(--transition-fast);
  }
  .stop-all svg {
    fill: var(--color-error);
  }
  .stop-all:hover {
    border-color: var(--color-error);
  }
  .stop-all:disabled {
    cursor: default;
    opacity: 0.6;
  }
  .band {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3) var(--space-6);
    margin: 0 0 var(--space-8);
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--color-border);
    border-left: 4px solid var(--color-error);
    border-radius: var(--radius-md);
    background: var(--color-background-surface);
  }
  .band p {
    margin: 0;
  }
  .band.failed {
    display: block;
  }
  .muted,
  .since {
    color: var(--color-text-muted);
  }
  strong {
    font-weight: 600;
  }
  .actions {
    display: flex;
    gap: var(--space-2);
  }
  .actions button {
    font: inherit;
    font-weight: 600;
    border-radius: var(--radius-md);
    padding: var(--space-2) var(--space-4);
    cursor: pointer;
  }
  .resume {
    color: var(--color-on-primary);
    background: var(--color-primary);
    border: 1px solid var(--color-primary);
  }
  .again {
    color: var(--color-text);
    background: none;
    border: 1px solid var(--color-border);
  }
  .actions button:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .note {
    margin: 0 0 var(--space-8);
    color: var(--color-text-muted);
  }
  @media (max-width: 600px), (pointer: coarse) {
    .stop-all,
    .actions button {
      min-height: var(--size-touch);
      min-width: var(--size-touch);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .stop-all {
      transition: none;
    }
  }
</style>
