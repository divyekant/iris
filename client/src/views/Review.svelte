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
  // One queue for each read and each write of a request: a step starts when the step before it
  // ends, and applies its answer before the next step starts. So the screen applies the answers in
  // the order that the store made them, and an older document never replaces a newer one. This
  // holds for the poll, the read of a state, and the answer of a decision or a withdraw (issue 184).
  let chain: Promise<unknown> = Promise.resolve();
  let queued = 0;
  function queue<T>(work: () => Promise<T>, apply: (out: T) => void): Promise<T> {
    queued++;
    const step = chain.then(async () => {
      try {
        const out = await work();
        apply(out);
        return out;
      } finally {
        queued--;
      }
    });
    chain = step.catch(() => {});
    return step;
  }
  // What can change on the screen: the revision, the state, and the decision of each action.
  const mark = (c: Changeset) => `${c.revision}|${c.state}|${c.actions.map((a) => a.decision?.state ?? "").join(",")}`;
  // A Request keeps its part (its ticks and pages): a poll replaces only a changed document. The
  // answer to a read or a write of the Request itself always replaces it (`asked`), because the
  // Request loads its lines again for each new document.
  function put(d: Changeset, asked = false) {
    unknown = unknown.filter((x) => x !== d.id);
    const shownAs = all.find((c) => c.id === d.id);
    if (shownAs) {
      if (asked || mark(shownAs) !== mark(d)) now[d.id] = d;
    } else later = later.map((c) => (c.id === d.id ? d : c));
  }
  // A Request sends its reads and writes of the request through the queue.
  const read = (work: () => Promise<Changeset>) => queue(work, (d) => put(d, true));

  function poll(open: Changeset[]) {
    if (!Array.isArray(open)) return;
    for (const c of open) put(c);
    order = open.map((c) => c.id);
    // A new request waits in `later` until Show. An announced request stays there, also when it closes.
    const fresh = open.filter((c) => !list.some((x) => x.id === c.id) && !later.some((x) => x.id === c.id));
    later = [...later, ...fresh];
    // A request on the screen that left the open list goes to the end. Each keeps its part.
    const moved = [...list.filter((c) => order.includes(c.id)), ...list.filter((c) => !order.includes(c.id))];
    if (moved.some((c, i) => c !== list[i])) list = moved;
    // A request on the screen or announced that the engine no longer lists as open: read its state,
    // so that it never keeps the controls of an open request.
    for (const c of [...all, ...later]) if (c.state === "open" && !order.includes(c.id)) void settle(c.id);
  }
  $effect(() => {
    if (!every) return;
    const timer = setInterval(() => {
      // The poll waits for a quiet queue, so that it never piles up behind a slow request.
      if (globalThis.document?.visibilityState === "hidden" || queued > 0) return;
      queue(() => get<{ changesets: Changeset[] }>("changesets"), (r) => poll(r.changesets)).catch(() => {
        // The next read tries again.
      });
    }, every);
    return () => clearInterval(timer);
  });

  // The requests that left the open list and whose state Iris could not read. Each is locked
  // (inert) with a line and Try again, and the headline does not count it. The next poll tries again.
  let unknown: string[] = $state([]);
  function settle(id: string) {
    const step = async () => {
      try {
        return await get<Changeset>(`changesets/${encodeURIComponent(id)}`);
      } catch {
        return id;
      }
    };
    return queue(step, (d) => (typeof d === "string" ? !unknown.includes(d) && (unknown = [...unknown, d]) : put(d)));
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
  // Only an open request takes a decision: a draft revision, a withdrawn or a closed request does not count.
  let asks = $derived(all.filter((c) => c.state === "open" && !unknown.includes(c.id) && shown(c).some(waits)));
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
      <button type="button" onclick={() => settle(c.id)}>Try again</button>
    </p>
  {/if}
  {#if c.state === "draft"}
    <p class="later" role="status">{author(c)} changes this request. You can decide when they submit the new version.</p>
  {/if}
  <div class="one" data-changeset={c.id} inert={c.state === "draft" || unknown.includes(c.id)}><Request changeset={c} {read} /></div>
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
