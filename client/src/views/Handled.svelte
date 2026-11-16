<!--
  What Iris did without you (card R2-09): the changes that a rule approved,
  and the items that a rule hides from the brief, each with its reason.
  An approved change can still wait to apply: each row says its state.
  The brief shows the count. This view is the one step to the list.
-->
<script lang="ts">
  import { get, own, type HandledAction, type HandledPage, type Page } from "../api";
  import { addPage, when } from "../format";
  import { verb } from "../review";
  import Line from "./Line.svelte";

  let { changes: firstChanges, hidden: firstHidden }: { changes: HandledPage; hidden: Page & { count: number } } = $props();

  // svelte-ignore state_referenced_locally
  let actions = $state(firstChanges.actions);
  // svelte-ignore state_referenced_locally
  let nextAction = $state(firstChanges.next);
  // svelte-ignore state_referenced_locally
  let items = $state(firstHidden.items);
  // svelte-ignore state_referenced_locally
  let nextItem = $state(firstHidden.next);
  // A possible duplicate shows with its original (spec 1 section 7.5), as in each list of the brief.
  // svelte-ignore state_referenced_locally
  let duplicates = $state(firstHidden.duplicates ?? {});
  let error = $state("");
  let busy = $state("");

  const n = (count: number, one: string, many: string) => `${count.toLocaleString("en-US")} ${count === 1 ? one : many}`;
  const STATE = {
    approved: "Approved, waits to apply",
    held: "On hold",
    applied: "Done",
    failed: "Failed. Some messages can have changed",
    undone: "Undone",
    denied: "Denied",
  } as const;
  // The review words of the verb need the parts of an action that the list gives.
  const words = (a: HandledAction) => verb({ verb: a.verb, args: a.args } as Parameters<typeof verb>[0]);
  const key = (a: HandledAction) => `${a.changeset}/${a.revision}/${a.n}`;

  async function more(which: "changes" | "hidden") {
    busy = which;
    error = "";
    try {
      if (which === "changes") {
        const page = await get<HandledPage>(`handled?limit=50&after=${encodeURIComponent(String(nextAction ?? ""))}`);
        const seen = new Set(actions.map(key));
        actions = [...actions, ...page.actions.filter((a) => !seen.has(key(a)))];
        nextAction = page.next;
      } else {
        const page = await get<Page>(`brief/hidden?limit=50&after=${encodeURIComponent(String(nextItem ?? ""))}`);
        items = addPage(items, page.items);
        duplicates = { ...duplicates, ...page.duplicates };
        nextItem = page.next;
      }
    } catch (e) {
      error = own(e);
    } finally {
      busy = "";
    }
  }
</script>

<h1>Handled without you</h1>
<p class="lead">Your rules decided these things, so Iris did not ask you. Each one says why, and if it is done yet. Hidden mail is never deleted: it stays in your mailbox.</p>

{#if firstChanges.count === 0 && firstHidden.count === 0}
  <p class="empty">Iris has handled nothing without you yet. <a href="#/rules">Your rules</a> say what it can do.</p>
{/if}

{#if firstChanges.count > 0}
  <section aria-labelledby="changes">
    <h2 id="changes"><span>Decided by your rules</span> <span class="count">{firstChanges.count}</span></h2>
    <ul>
      {#each actions as a (key(a))}
        <li class="change">
          <p class="what"><strong>{words(a)}</strong> {n(a.messages, "message", "messages")}</p>
          {#if a.reason || a.why}<p class="why">{a.reason || a.why}</p>{/if}
          <p class="meta">{STATE[a.state] ?? a.state}{#if a.at}, {when(a.at)}{/if}</p>
        </li>
      {/each}
    </ul>
    {#if nextAction}<button type="button" onclick={() => more("changes")} disabled={!!busy}>{busy === "changes" ? "Loading" : "Show more"}</button>{/if}
  </section>
{/if}

{#if firstHidden.count > 0}
  <section aria-labelledby="hidden">
    <h2 id="hidden"><span>Hidden from your brief</span> <span class="count">{firstHidden.count}</span></h2>
    <ul>
      {#each items as item (item.id)}<Line {item} twins={duplicates[item.id]} note={item.attention?.reason ?? ""} />{/each}
    </ul>
    {#if nextItem}<button type="button" onclick={() => more("hidden")} disabled={!!busy}>{busy === "hidden" ? "Loading" : "Show more"}</button>{/if}
  </section>
{/if}
{#if error}<p role="alert">{error}</p>{/if}

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
  section {
    margin-bottom: var(--space-12);
  }
  h2 {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    font-size: var(--font-size-0);
    font-weight: 600;
    margin: 0 0 var(--space-2);
    padding-bottom: var(--space-2);
    border-bottom: 1px solid var(--color-border);
  }
  .count {
    color: var(--color-text-muted);
    font-weight: 400;
    font-variant-numeric: tabular-nums;
  }
  ul {
    list-style: none;
    margin: 0 0 var(--space-4);
    padding: 0;
  }
  .change {
    padding: var(--space-3) 0;
    border-bottom: 1px solid var(--color-border-subtle);
  }
  .change p {
    margin: 0;
  }
  .what strong {
    font-weight: 600;
  }
  .why {
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
    margin-top: var(--space-1);
  }
  .meta {
    color: var(--color-text-faint);
    font-size: var(--font-size--1);
  }
  button {
    font: inherit;
    color: var(--color-text);
    background: var(--color-background-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    padding: var(--space-2) var(--space-4);
    cursor: pointer;
  }
  button:hover {
    border-color: var(--color-primary);
  }
</style>
