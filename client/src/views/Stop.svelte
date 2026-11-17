<!--
  One Stop for all agent work (card R4-11, docs/specs/2026-11-17-stop.md).
  In the header: the Stop button. While Iris is stopped: a band above each
  view that says so, with Resume. A stop needs no confirmation: it changes
  no mail, and Resume undoes it.
-->
<script lang="ts">
  import { post, own } from "../api";
  import { when } from "../format";

  type State = { stopped: boolean; since: string | null };
  type Stopped = State & { sessions: number; live: number; busy: boolean };

  let { place }: { place: "button" | "band" } = $props();

  /** The time of the stop: only the clock time on the same day. */
  function since(value: string): string {
    const at = new Date(value);
    return at.toDateString() === new Date().toDateString() ? at.toLocaleTimeString(undefined, { timeStyle: "short" }) : when(value);
  }
  const shared = stopState;
</script>

<script module lang="ts">
  import { get as read } from "../api";

  type Shared = { stopped: boolean; since: string | null; live: number; busy: boolean; busyAction: "" | "stop" | "resume"; note: string; error: string };
  /** One state for the button and the band, read from GET /api/stop. */
  export const stopState: Shared = $state({ stopped: false, since: null, live: 0, busy: false, busyAction: "", note: "", error: "" });

  /** Read the state again: at start, at each new view and when the tab comes back. */
  export async function refresh() {
    try {
      const s = await read<{ stopped: boolean; since: string | null; live: number; busy: boolean }>("stop");
      Object.assign(stopState, { stopped: s.stopped, since: s.since, live: s.stopped ? s.live : 0, busy: s.stopped && s.busy });
    } catch {
      // The access state shows a failure of the engine above each view.
    }
  }
</script>

{#if place === "button" && !shared.stopped}
  <button
    class="stop-all"
    type="button"
    disabled={shared.busyAction === "stop"}
    aria-label={shared.busyAction === "stop" ? "Stopping all agent work" : "Stop all agent work"}
    title="Stop all agent work. Nothing applies and no mail leaves until you resume."
    onclick={async () => {
      shared.busyAction = "stop";
      shared.error = "";
      shared.note = "";
      try {
        const s = await post<Stopped>("stop", {});
        Object.assign(shared, { stopped: true, since: s.since, live: s.live, busy: s.busy });
      } catch (e) {
        shared.error = own(e) || "Iris did not stop. Try again, or run iris stop at the terminal.";
      } finally {
        shared.busyAction = "";
      }
    }}
  >
    <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true"><rect x="2" y="2" width="12" height="12" rx="2" /></svg>
  </button>
{/if}

{#if place === "band"}
  {#if shared.error}<p class="band failed" role="alert">{shared.error}</p>{/if}
  {#if shared.stopped}
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
        {#if shared.live > 0}
          <button
            class="again"
            type="button"
            disabled={shared.busyAction !== ""}
            onclick={async () => {
              shared.busyAction = "stop";
              try {
                const s = await post<Stopped>("stop", {});
                Object.assign(shared, { since: s.since, live: s.live, busy: s.busy });
              } catch (e) {
                shared.error = own(e) || "Iris did not answer. Try again.";
              } finally {
                shared.busyAction = "";
              }
            }}>Stop again</button>
        {/if}
        <button
          class="resume"
          type="button"
          disabled={shared.busyAction !== ""}
          onclick={async () => {
            shared.busyAction = "resume";
            shared.error = "";
            try {
              await post<State>("resume", {});
              Object.assign(shared, { stopped: false, since: null, live: 0, busy: false, note: "Iris runs again. Each held send waits a full minute before it leaves." });
            } catch (e) {
              shared.error = own(e) || "Iris did not resume. Try again.";
            } finally {
              shared.busyAction = "";
            }
          }}>{shared.busyAction === "resume" ? "Resuming" : "Resume"}</button>
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
