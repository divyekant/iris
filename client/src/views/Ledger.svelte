<!--
  What Iris did (card R2-07, option A): each ledger entry, newest first,
  under its day. Each line says what changed, why, who decided it and when,
  and has its own Undo while an undo can start (specification 2, section 7.4).
  The engine applies an undo at its next step. The line says that Iris undoes
  the change on each message or item that did not change again or go, and the view
  reads the list again until no undo or change is open.
-->
<script lang="ts">
  import { get, own, post, type LedgerEntry, type LedgerPage } from "../api";
  import { byDay, did, status, time, what, who } from "../ledger";

  // `wait`: the time between two reads of the list while an undo or a change is open.
  let { page, wait = 3000 }: { page: LedgerPage; wait?: number } = $props();

  // svelte-ignore state_referenced_locally
  let entries = $state(page.entries);
  // svelte-ignore state_referenced_locally
  let next = $state(page.next);
  let busy = $state("");
  let loading = $state(false);
  let problem = $state("");
  let groups = $derived(byDay(entries));
  let times = $derived(new Map(entries.map((e) => [e.id, time(e.at)])));
  let open = $derived(entries.some((e) => e.undo_open || e.state === "writing"));
  let reads = $state(0);

  // Each change to the list (read again, Show more, Undo) runs in turn, never two at the same
  // time, so that each one starts from the list that the one before left, and no older answer
  // writes over a newer list or cursor.
  let last: Promise<unknown> = Promise.resolve();
  function inTurn<T>(work: () => Promise<T>): Promise<T> {
    const p = last.then(work);
    last = p.catch(() => {});
    return p;
  }

  // While an undo or a change is open, read the list again, so that each line shows its end.
  $effect(() => {
    void reads;
    if (!open) return;
    const timer = setTimeout(() => inTurn(fresh).catch(() => {}).finally(() => reads++), wait);
    return () => clearTimeout(timer);
  });

  const row = (id: string) => Number(id.slice(2));

  // Read the list again from the start, as far as the oldest loaded entry, so that each
  // loaded line is as the engine says now, and a new entry (an undo) shows at the top.
  async function fresh() {
    const oldest = entries.at(-1);
    let list: LedgerEntry[] = [];
    let after: string | null = null;
    for (;;) {
      const p: LedgerPage = await get<LedgerPage>(`ledger?limit=200${after ? `&after=${encodeURIComponent(after)}` : ""}`);
      list = [...list, ...p.entries];
      after = p.next;
      if (!after || (oldest && row(list.at(-1)!.id) <= row(oldest.id))) break;
    }
    // Entries never leave the ledger, so the next page after the oldest loaded entry stays the same.
    if (oldest) list = list.filter((e) => row(e.id) >= row(oldest.id));
    else next = after;
    entries = list;
  }

  const MARK: Record<string, string> = {
    archive: "box",
    inbox: "box",
    trash: "bin",
    untrash: "bin",
    label: "tag",
    unlabel: "tag",
    read: "mail",
    unread: "mail",
    send: "mail",
    draft: "mail",
  };

  // The reason of an undo is the text of the engine ("Undo of l-12"): say it in words.
  function why(e: LedgerEntry): string {
    if (e.undo_of) return times.has(e.undo_of) ? `The undo of the change at ${times.get(e.undo_of)}.` : "The undo of an earlier change.";
    return e.why ?? "";
  }

  async function more() {
    loading = true;
    problem = "";
    try {
      await inTurn(async () => {
        const p = await get<LedgerPage>(`ledger?limit=50&after=${encodeURIComponent(next ?? "")}`);
        const seen = new Set(entries.map((e) => e.id));
        entries = [...entries, ...p.entries.filter((e) => !seen.has(e.id))];
        next = p.next;
      });
    } catch (e) {
      problem = own(e);
    } finally {
      loading = false;
    }
  }

  async function reload(why: string) {
    try {
      await fresh();
      problem = why;
    } catch {
      problem = `${why} Iris could not load the list again. Load the page again.`;
    }
  }

  async function undo(e: LedgerEntry) {
    busy = e.id;
    problem = "";
    try {
      await inTurn(async () => {
        try {
          const after = await post<LedgerEntry>(`ledger/${e.id}/undo`, {});
          entries = entries.map((x) => (x.id === e.id ? after : x));
        } catch (err) {
          const status = (err as { status?: number }).status;
          // A 409 is a definite refusal: nothing was written. With no answer or a 5xx, the result is unknown.
          if (status === 409) await reload(`Iris did not start the undo: ${own(err)} Here is the list as it is now.`);
          else if (status === undefined || status >= 500) await reload("Iris does not know if the undo started. Here is the list as it is now.");
          else problem = own(err);
        }
      });
    } finally {
      busy = "";
    }
  }
</script>

<h1>What Iris did</h1>
<p class="lead">Each change that Iris made to your mail, newest first. Undo puts back each message or item that did not change again since.</p>
{#if problem}<p role="alert">{problem}</p>{/if}

{#if entries.length === 0}
  <p class="empty">Iris has changed nothing yet. When you approve a change, or <a href="#/rules">a rule</a> allows one, it shows here.</p>
{/if}

{#each groups as g (g.day + g.entries[0].id)}
  <h2>{g.day}</h2>
  <ul>
    {#each g.entries as e (e.id)}
      {@const s = status(e)}
      <li class:gone={e.state === "undone"}>
        <span class="mark" aria-hidden="true">
          <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
            {#if MARK[e.verb] === "box"}<rect x="3" y="4" width="18" height="5" rx="1" /><path d="M5 9v10h14V9M10 13h4" />
            {:else if MARK[e.verb] === "bin"}<path d="M4 7h16M9 7V4h6v3M6 7l1 13h10l1-13" />
            {:else if MARK[e.verb] === "tag"}<path d="M3 12V4h8l10 10-8 8z" /><circle cx="7.5" cy="8.5" r="1.2" />
            {:else if MARK[e.verb] === "mail"}<rect x="3" y="5" width="18" height="14" rx="1" /><path d="M3 7l9 6 9-6" />
            {:else}<path d="M5 12l5 5 9-10" />{/if}
          </svg>
        </span>
        <div class="body">
          <p><strong>{did(e)}</strong> {what(e)}</p>
          {#if why(e)}<p class="why">{why(e)}</p>{/if}
          {#if s}<p class="why state"><span class="dot" class:warn={e.state === "failed"}></span>{s}</p>{/if}
          <p class="meta">{who(e)} · <time datetime={e.at}>{time(e.at)}</time>{#if e.state === "undone"}{" · Undone"}{/if}</p>
          {#if e.undo_open}
            <p class="going"><span class="dot"></span>Iris undoes the change. {e.items > 0 ? "An item" : "A message"} that changed again, or that is gone, stays as it is. This can take a minute.</p>
          {/if}
        </div>
        {#if e.can_undo && !e.undo_open}
          <button type="button" onclick={() => undo(e)} disabled={!!busy} aria-label={`Undo: ${did(e)} ${what(e)}`}>{busy === e.id ? "Undoing" : "Undo"}</button>
        {/if}
      </li>
    {/each}
  </ul>
{/each}
{#if next}<button type="button" class="more" onclick={more} disabled={loading}>{loading ? "Loading" : "Show more"}</button>{/if}

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
  .empty a {
    color: var(--color-text-muted);
  }
  h2 {
    font-size: var(--font-size--1);
    font-weight: 600;
    color: var(--color-text-faint);
    text-transform: uppercase;
    letter-spacing: 0.06em;
    margin: var(--space-8) 0 var(--space-1);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  li {
    display: flex;
    gap: var(--space-3);
    align-items: flex-start;
    padding: var(--space-3) 0;
    border-bottom: 1px solid var(--color-border-subtle);
  }
  li.gone {
    opacity: 0.55;
  }
  .mark {
    flex: none;
    width: 32px;
    height: 32px;
    border-radius: var(--radius-full);
    background: var(--color-background-surface);
    color: var(--color-text-muted);
    display: grid;
    place-items: center;
  }
  .body {
    flex: 1;
    min-width: 0;
  }
  .body p {
    margin: 0;
  }
  strong {
    font-weight: 600;
  }
  .why {
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  .meta {
    color: var(--color-text-faint);
    font-size: var(--font-size--1);
    font-variant-numeric: tabular-nums;
  }
  .state,
  .going {
    display: flex;
    align-items: baseline;
    gap: var(--space-2);
  }
  .going {
    margin-top: var(--space-2);
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    border-radius: var(--radius-full);
    background: var(--color-primary);
    transform: translateY(-1px);
  }
  .dot.warn {
    background: var(--color-warning);
  }
  .state .dot:not(.warn) {
    background: var(--color-text-faint);
  }
  button {
    flex: none;
    font: inherit;
    font-size: var(--font-size--1);
    color: var(--color-text);
    background: none;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-full);
    padding: var(--space-1) var(--space-3);
    cursor: pointer;
    transition: color var(--transition-fast), border-color var(--transition-fast);
  }
  button:hover:not(:disabled),
  button:focus-visible {
    border-color: var(--color-primary);
    color: var(--color-primary);
  }
  button:disabled {
    cursor: default;
    opacity: 0.6;
  }
  .more {
    margin-top: var(--space-4);
    font-size: var(--font-size-0);
    border-radius: var(--radius-md);
    padding: var(--space-2) var(--space-4);
    background: var(--color-background-elevated);
  }
  @media (max-width: 600px) {
    .mark {
      width: 28px;
      height: 28px;
    }
  }
</style>
