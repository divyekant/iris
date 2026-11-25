<!--
  The mail that the agent of a job reads (decision D25): each thread and item
  of the scope, with "Remove" on an open job. "Add mail" searches the mail; a
  press on a thread shows each message that the add grants, then "Add this
  thread". The add sends the IDs that the person saw: if the thread changed,
  the engine refuses it and the preview loads again.
-->
<script lang="ts">
  import { get, own, type JobDetail, type Summary } from "../api";
  import { when } from "../format";
  import { addThread, loadJob, removeRef } from "../jobs";
  import { byThread, reads, subjectOf, threadsOf, type Preview } from "../start";

  let {
    detail,
    onchange,
    onbusy = () => {},
  }: {
    detail: JobDetail;
    onchange: (d: JobDetail) => void;
    /** True when a request of this part starts, false when it ends (before `onchange`). */
    onbusy?: (on: boolean) => void;
  } = $props();

  let job = $derived(detail.job);
  let open = $derived(job.state !== "done");
  let scope = $derived(detail.scope ?? []);
  let adding = $state(false);
  let query = $state("");
  let hits = $state<{ thread: string; subject: string; hits: number }[] | null>(null);
  let picked = $state<{ thread: string; preview: Preview } | null>(null);
  let busy = $state(false);
  let note = $state("");

  // True when the last failure had no answer of the engine: the change can have arrived.
  let unknown = false;
  async function run<T>(work: () => Promise<T>): Promise<T | undefined> {
    busy = true;
    note = "";
    unknown = false;
    onbusy(true);
    try {
      return await work();
    } catch (e) {
      note = own(e) || (e as Error).message;
      unknown = (e as { status?: number }).status === undefined;
    } finally {
      busy = false;
      onbusy(false);
    }
  }

  // The search gives the newest matches first. "More results" asks for more, up to the limit of the API.
  const STEP = 50;
  const MOST = 200;
  let limit = $state(STEP);
  let full = $state(false);
  let searched = "";

  async function search(event: SubmitEvent) {
    event.preventDefault();
    const q = query.trim();
    if (!q) return;
    await find(q, STEP);
  }

  async function find(q: string, n: number) {
    picked = null;
    const page = await run(() => get<{ messages: Summary[] }>(`search?q=${encodeURIComponent(q)}&limit=${n}`));
    if (!page) return;
    searched = q;
    limit = n;
    full = page.messages.length >= n;
    hits = threadsOf(page.messages);
  }

  async function pick(thread: string) {
    const preview = await run(() => get<Preview>(`jobs/${encodeURIComponent(job.id)}/scope/${encodeURIComponent(thread)}`));
    if (preview) picked = { thread, preview };
  }

  async function add() {
    if (!picked) return;
    const { thread, preview } = picked;
    const before = scope.some((r) => r.id === thread);
    const d = await run(() => addThread(job.id, thread, preview.messages.map((m) => m.id)));
    if (d) {
      onchange(d);
      close();
      return;
    }
    // With no answer, the add can have arrived. Only a new ref proves it: a thread that was in
    // the scope before can miss the new mail, so its add stays not done.
    const lost = unknown;
    const fresh = await refresh();
    if (lost && !before && fresh?.scope?.some((r) => r.id === thread)) {
      note = "";
      close();
      return;
    }
    // The mail changed after the preview: show the thread as it is now, and keep the note.
    const kept = note;
    await pick(thread);
    note = kept;
  }

  async function remove(ref: string) {
    const d = await run(() => removeRef(job.id, ref));
    if (d) return onchange(d);
    // The remove can have arrived with no answer: when the ref is gone now, it worked.
    const fresh = await refresh();
    if (fresh && !fresh.scope?.some((r) => r.id === ref)) note = "";
  }

  /** Read the job again after a change whose result is not known, and show it. */
  async function refresh(): Promise<JobDetail | null> {
    const fresh = await loadJob(job.id).catch(() => null);
    if (fresh) onchange(fresh);
    return fresh;
  }

  function close() {
    adding = false;
    query = "";
    hits = null;
    picked = null;
  }
</script>

<section aria-labelledby="scope">
  <h2 id="scope"><span>Mail the agent reads</span></h2>
  {#if scope.length === 0}
    <p class="empty">No mail. The agent reads only the mail that you add here.</p>
  {:else}
    <ul class="refs">
      {#each scope as r, i (r.id ?? `row-${i}`)}
        <li>
          <span class="what">
            <span class="ref-label">{r.label?.trim() || (r.kind === "thread" ? "(no subject)" : "(no title)")}</span>
            <span class="count">{r.messages.toLocaleString("en-US")} {r.messages === 1 ? "message" : "messages"}</span>
          </span>
          {#if open && r.id}<button class="quiet" type="button" disabled={busy} onclick={() => remove(r.id!)}>Remove</button>{/if}
        </li>
      {/each}
    </ul>
  {/if}

  {#if open && !adding}
    <button class="line" type="button" onclick={() => (adding = true)}>Add mail</button>
  {/if}

  {#if adding}
    <div class="add">
      <form class="search" onsubmit={search}>
        <label class="hidden" for="scope-search">Search the mail</label>
        <!-- svelte-ignore a11y_autofocus -->
        <input id="scope-search" type="search" bind:value={query} placeholder="Search the mail" autofocus />
        <button class="line" type="submit" disabled={busy || !query.trim()}>Search</button>
        <button class="quiet" type="button" onclick={close}>Cancel</button>
      </form>

      {#if picked}
        {@const g = byThread(picked.preview.messages)}
        <p class="label">The agent will also read {reads(picked.preview.messages)}.</p>
        {#each g as t (t.thread)}
          <div class="thread">
            <p class="subject">{t.subject}</p>
            <ul class="msgs">
              {#each t.messages as m (m.id)}
                <li>
                  <span class="who">{m.from.name ?? m.from.address ?? "Unknown sender"}</span>
                  <span class="when">{m.date ? when(m.date) : ""}</span>
                  {#if !m.readable}<span class="hidden-note">Hidden from the agent for now</span>{/if}
                  {#if subjectOf(m) !== t.subject}<span class="other">{subjectOf(m)}</span>{/if}
                </li>
              {/each}
            </ul>
          </div>
        {/each}
        <div class="row">
          <button class="go" type="button" disabled={busy} onclick={add}>Add this thread</button>
          <button class="quiet" type="button" onclick={() => (picked = null)}>Back</button>
        </div>
      {:else if hits}
        {#if hits.length === 0}
          <p class="empty">No mail matches.</p>
        {:else}
          <ul class="hits">
            {#each hits as h (h.thread)}
              <li><button class="hit" type="button" disabled={busy} onclick={() => pick(h.thread)}>{h.subject}</button></li>
            {/each}
          </ul>
          {#if full && limit < MOST}
            <button class="quiet" type="button" disabled={busy} onclick={() => find(searched, Math.min(limit + STEP * 2, MOST))}>More results</button>
          {:else if full}
            <p class="empty">These are the threads of the newest {MOST} matches. Add words to the search to find an older thread.</p>
          {/if}
        {/if}
      {/if}
    </div>
  {/if}
  {#if note}<p class="error" role="alert">{note}</p>{/if}
</section>

<style>
  section {
    margin: 0 0 var(--space-12);
  }
  h2 {
    font-size: var(--font-size-0);
    font-weight: 600;
    margin: 0 0 var(--space-2);
    padding-bottom: var(--space-2);
    border-bottom: 1px solid var(--color-border);
  }
  p {
    margin: 0;
  }
  .empty,
  .label {
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
    margin: var(--space-2) 0;
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .refs li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    padding: var(--space-2) 0;
    border-bottom: 1px solid var(--color-border-subtle);
  }
  .what {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .ref-label {
    overflow-wrap: anywhere;
  }
  .count,
  .when,
  .other,
  .hidden-note {
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  .add {
    display: grid;
    gap: var(--space-3);
    margin-top: var(--space-3);
    padding: var(--space-4);
    background: var(--color-background-elevated);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-lg);
  }
  .search,
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
  }
  input {
    flex: 1 1 200px;
    font: inherit;
    color: var(--color-text);
    background: var(--color-background);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    min-height: calc(var(--space-8) + var(--space-2));
    padding: 0 var(--space-3);
    color-scheme: inherit;
  }
  input:focus {
    outline: none;
    border-color: var(--color-primary);
  }
  .hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }
  .hit {
    display: block;
    width: 100%;
    text-align: left;
    padding: var(--space-2) 0;
    border: 0;
    border-bottom: 1px solid var(--color-border-subtle);
    border-radius: 0;
    background: transparent;
    color: var(--color-text);
  }
  .hit:hover {
    color: var(--color-primary);
  }
  .subject {
    font-weight: 500;
  }
  .msgs {
    margin-top: var(--space-1);
    padding-left: var(--space-4);
    border-left: 1px solid var(--color-border);
  }
  .msgs li {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1) var(--space-3);
    padding: var(--space-1) 0;
    font-size: var(--font-size--1);
  }
  .other {
    flex-basis: 100%;
  }
  button {
    font: inherit;
    cursor: pointer;
    min-height: calc(var(--space-8) + var(--space-2));
    border-radius: var(--radius-full);
    transition: background var(--transition-fast), color var(--transition-fast);
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .line {
    margin-top: var(--space-2);
    padding: 0 var(--space-4);
    color: var(--color-text);
    background: var(--color-background-elevated);
    border: 1px solid var(--color-border);
  }
  .search .line {
    margin-top: 0;
  }
  .line:hover {
    border-color: var(--color-primary);
  }
  .go {
    padding: 0 var(--space-6);
    border: 0;
    background: var(--color-primary);
    color: var(--color-on-primary);
    font-weight: 600;
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
  .error {
    margin-top: var(--space-2);
    border-left: 4px solid var(--color-error);
    padding-left: var(--space-3);
  }
</style>
