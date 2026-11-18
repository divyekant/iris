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
   * One state for the button and the band. Only a read of GET /api/stop sets
   * it. `known` is true only when no Stop or Resume is open and the newest
   * read agrees with the last action that the person asked for.
   */
  export const stopState: Shared = $state({ stopped: false, since: null, live: 0, busy: false, known: true, busyAction: "", note: "", error: "" });

  // The number of the newest read. Only the newest read changes the state.
  let newest = 0;
  // The count of Stop and Resume requests with no answer yet. A write can
  // commit before its answer comes, so while one is open nothing is known.
  let open = 0;
  // The last action that the person asked for, until a read shows it. A
  // failed action can still commit after a read, so it stays until then.
  let wanted: "" | "stop" | "resume" = "";

  function show(n: number, a: Answer) {
    if (n !== newest) return;
    const known = open === 0 && (!wanted || a.stopped === (wanted === "stop"));
    if (known) wanted = "";
    Object.assign(stopState, { stopped: a.stopped, since: a.since, live: a.stopped ? (a.live ?? 0) : 0, busy: a.stopped && !!a.busy, known });
    if (known) stopState.error = "";
  }

  /** Read the state from the engine. With no answer, the state is not known. */
  export async function reread() {
    const n = ++newest;
    try {
      show(n, await read<Answer>("stop"));
    } catch {
      // The access state shows the failure of the engine above each view.
      if (n === newest) stopState.known = false;
    }
  }

  /** The action to try again while the state is not known. */
  export function pending(): "" | "stop" | "resume" {
    return wanted;
  }

  /** Read the state again: at start, at each new view and when the tab comes back. */
  export async function refresh() {
    // An open Stop or Resume reads the state when it ends.
    if (open > 0) return;
    await reread();
  }

  /**
   * Stop all agent work, or resume it. A Stop can start while a Resume is
   * open, because a stop must always be possible. A Resume starts only when
   * nothing is open.
   */
  export async function act(kind: "stop" | "resume") {
    if (open > 0 && (kind === "resume" || wanted === "stop")) return;
    wanted = kind;
    open++;
    newest++; // A read that is open now is older than this action.
    Object.assign(stopState, { known: false, busyAction: kind, error: "", note: "" });
    let resumed = false;
    try {
      const a = await post<Answer>(kind, {});
      resumed = kind === "resume" && !!a.resumed;
    } catch (e) {
      if (wanted === kind) {
        stopState.error = own(e) || (kind === "stop" ? "Iris did not answer the Stop. Try again, or run iris stop at the terminal." : "Iris did not answer the Resume. Try again.");
      }
    }
    open--;
    if (open > 0) return;
    stopState.busyAction = "";
    await reread();
    // Only a real resume starts a new hold of each held send.
    if (resumed && stopState.known && !stopState.stopped) stopState.note = "Iris runs again. Each held send waits a full minute before it leaves.";
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
      <p class="words">{shared.busyAction ? "Iris waits for the answer of the engine." : "Iris does not know if agent work is stopped now."}</p>
      <div class="actions">
        {#if shared.error && !shared.busyAction && pending()}
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
          <p class="muted">No agent runs, no approved action applies and no mail leaves.</p>
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
