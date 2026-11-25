<!--
  The review screen (R2-06, spec 2 section 5.5): each open changeset, so that
  the person approves or denies what an agent or Iris asks to change. The screen
  reads the open changesets each minute, and a new request waits for Show.
-->
<script lang="ts">
  import { get, type Changeset, type Mode } from "../api";
  import { author, shown, waits } from "../review";
  import Request from "./Request.svelte";

  // `every`: the ms between two reads of the open changesets (a prop for the tests; 0 reads none).
  let { changesets, mode = "apply", every = 60_000 }: { changesets: Changeset[]; mode?: Mode; every?: number } = $props();

  // The changesets on the screen. A read never replaces one: a Request keeps its ticks and its
  // loaded pages. A new request waits in `later` until the person presses Show (issue 184).
  // svelte-ignore state_referenced_locally
  let list = $state(changesets);
  let later: Changeset[] = $state([]);
  let order: string[] = [];
  // Each read takes a number at its start. An answer applies only if no later read applied first.
  let started = 0;
  let applied = 0;
  $effect(() => {
    if (!every) return;
    const timer = setInterval(async () => {
      if (globalThis.document?.visibilityState === "hidden") return;
      const n = ++started;
      try {
        const open = (await get<{ changesets: Changeset[] }>("changesets")).changesets;
        if (n <= applied || !Array.isArray(open)) return;
        applied = n;
        // An announced request stays in `later` until Show, also when it closes before then.
        const fresh = open.filter((c) => !list.some((x) => x.id === c.id) && !later.some((x) => x.id === c.id));
        later = [...later.map((c) => open.find((x) => x.id === c.id) ?? c), ...fresh];
        order = open.map((c) => c.id);
        // A request on the screen that is open again (a new revision after a draft): take the new document.
        for (const c of open) {
          const shownAs = all.find((x) => x.id === c.id);
          if (shownAs && (shownAs.state !== "open" || unknown.includes(c.id))) now[c.id] = c;
        }
        unknown = unknown.filter((id) => !order.includes(id));
        // A request on the screen or announced that the engine no longer lists as open: read its state,
        // so that it never keeps the controls of an open request.
        void settle([...all, ...later].filter((c) => c.state === "open" && !order.includes(c.id)).map((c) => c.id));
      } catch {
        // The next read tries again.
      }
    }, every);
    return () => clearInterval(timer);
  });

  // The requests that left the open list and whose state Iris could not read. Each is locked
  // (inert) with a line and Try again, and the headline does not count it. The next poll tries again.
  let unknown: string[] = $state([]);
  async function settle(ids: string[]) {
    await Promise.all(
      ids.map(async (id) => {
        try {
          const d = await get<Changeset>(`changesets/${encodeURIComponent(id)}`);
          if (list.some((c) => c.id === id)) now[id] = d;
          else later = later.map((c) => (c.id === id ? d : c));
          unknown = unknown.filter((x) => x !== id);
        } catch {
          if (!unknown.includes(id)) unknown = [...unknown, id];
        }
      }),
    );
  }

  // Show the new requests in the order of the engine. A request that is no longer open stays at the end with its state.
  function showLater() {
    const known = [...list, ...later];
    list = [
      ...order.flatMap((id) => known.filter((c) => c.id === id)),
      ...list.filter((c) => !order.includes(c.id)),
      ...later.filter((c) => !order.includes(c.id)),
    ];
    later = [];
  }

  // Each open changeset shows, also one whose actions all have a decision. The headline counts only the waiting ones.
  // A changeset after a decision on this screen, by its ID. It takes the place of the one that the page loaded.
  let now: Record<string, Changeset> = $state({});
  let all = $derived(list.map((c) => now[c.id] ?? c));
  let asks = $derived(all.filter((c) => !unknown.includes(c.id) && shown(c).some(waits)));
  let count = $derived(asks.reduce((sum, c) => sum + shown(c).filter(waits).length, 0));
  let headline = $derived(
    count === 0
      ? "Nothing waits for your review."
      : asks.length === 1
        ? `${author(asks[0])} asks for ${count === 1 ? "one change" : `${count} changes`}.`
        : `${asks.length} requests ask for ${count} changes.`,
  );
</script>

<div class="opening">
  <p class="day">Review</p>
  <h1>{headline}</h1>
  {#if list.length === 0}
    <p class="empty">When an agent or Iris asks to change your mailbox, the request shows here with each message that it changes.</p>
  {/if}
</div>

{#if count > 0 && mode === "observe"}
  <p class="note">Iris is in observe mode. A change that you approve waits until you choose apply mode in <a href="#/rules">Rules</a>.</p>
{/if}

{#if later.length > 0}
  <p class="later" role="status">
    {later.length === 1 ? "1 new request" : `${later.length} new requests`} came in.
    <button type="button" onclick={showLater}>Show</button>
  </p>
{/if}

{#each all as c (c.id)}
  {#if unknown.includes(c.id)}
    <p class="later" role="status">
      This request is no longer open. Iris could not read its state.
      <button type="button" onclick={() => settle([c.id])}>Try again</button>
    </p>
  {/if}
  <div class="one" data-changeset={c.id} inert={unknown.includes(c.id)}><Request changeset={c} onchange={(d) => (now[d.id] = d)} /></div>
{/each}

<style>
  .opening {
    margin: var(--space-8) 0 var(--space-12);
  }
  .day,
  .empty {
    color: var(--color-text-muted);
    margin: 0 0 var(--space-3);
  }
  .empty {
    margin-top: var(--space-4);
    max-width: 48ch;
  }
  h1 {
    font-size: var(--font-size-4);
    font-weight: 500;
    line-height: 1.2;
    letter-spacing: -0.02em;
    margin: 0;
    max-width: 28ch;
    text-wrap: balance;
  }
  .one {
    display: contents;
  }
  .later {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-3);
    margin: 0 0 var(--space-8);
    padding: var(--space-2) var(--space-2) var(--space-2) var(--space-4);
    border: 1px solid var(--color-primary);
    border-radius: var(--radius-full);
    width: fit-content;
    max-width: 100%;
    box-sizing: border-box;
    color: var(--color-text);
  }
  .later button {
    padding: var(--space-1) var(--space-4);
    border: 0;
    border-radius: var(--radius-full);
    background: var(--color-primary);
    color: var(--color-on-primary);
    font: inherit;
    font-weight: 500;
    cursor: pointer;
  }
  .later button:focus-visible {
    outline: 2px solid var(--color-text);
    outline-offset: 2px;
  }
  @media (max-width: 600px), (any-pointer: coarse) {
    .later button {
      min-height: var(--size-touch);
    }
  }
  .note {
    margin: 0 0 var(--space-8);
    padding-left: var(--space-3);
    border-left: 4px solid var(--color-warning);
    color: var(--color-text-muted);
  }
</style>
