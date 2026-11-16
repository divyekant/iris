<!--
  One changeset in the review (R2-06, spec 2 section 5.5): who asks, why, and
  each action with its real targets (messages or items) and the arguments
  that set its effect. Read only: approve and deny come in the next part.
-->
<script lang="ts">
  import { get, type Changeset, type Item, type Target } from "../api";
  import { author, detail, FLAGS, itemRow, key, messageRow, outcome, pages, short, shown, target, targets, verb, waits, type Row } from "../review";

  let { changeset: c }: { changeset: Changeset } = $props();

  const FIRST = 3;
  const PAGE = 200;

  let actions = $derived(shown(c));
  let open = $derived(actions.filter(waits));
  let decided = $derived(actions.filter((a) => !waits(a)));
  let who = $derived(author(c));
  let mark = $derived(who.split(/\s+/).slice(0, 2).map((w) => w[0]?.toUpperCase()).join(""));

  // The targets of each action, loaded in pages: messages by their summaries, items one by one.
  const loader = pages(async (ids, items) => {
    if (!items) return (await get<{ messages: Target[] }>(`messages?ids=${ids.map(encodeURIComponent).join(",")}`)).messages.map(messageRow);
    return Promise.all(ids.map((id) => get<Item>(`items/${encodeURIComponent(id)}`).then((i) => itemRow(id, i), () => itemRow(id, null))));
  });
  let rows: Record<string, Row[]> = $state({});
  let problem = $state("");
  function load(a: (typeof actions)[number], count: number) {
    loader.more(a, count).then(
      (list) => list && (rows[key(a)] = list),
      (e) => (problem = e instanceof Error ? e.message : String(e)),
    );
  }
  $effect(() => {
    for (const a of actions) if (!(key(a) in rows)) load(a, FIRST);
  });
</script>

{#snippet lines(a: (typeof actions)[number])}
  {@const list = rows[key(a)] ?? []}
  {@const all = targets(a).length}
  {#if list.length}
    <ul class="messages">
      {#each list as m (m.id)}
        <li>
          {#if m.gone}
            <span class="gone">{a.targets.items?.length ? `${m.id}: not in Iris any more` : "A message that is not in your mailbox any more"}</span>
          {:else}
            <a href={m.href}>
              <span class="from">{m.who}</span>
              <span class="subject">{m.what}</span>
              <span class="date">{short(m.when)}</span>
            </a>
          {/if}
        </li>
      {/each}
    </ul>
  {/if}
  {#if all > list.length && list.length}
    <button type="button" class="more" onclick={() => load(a, PAGE)}>
      {all - list.length > PAGE ? `Show ${PAGE} more` : `Show all ${all.toLocaleString("en-US")}`}
    </button>
  {/if}
{/snippet}

<article class="request" aria-labelledby={`${c.id}-who`}>
  <header>
    {#if c.author.kind === "triage"}
      <!-- Iris asks: the mark of Iris. -->
      <svg class="mark" viewBox="0 0 24 24" aria-hidden="true">
        <circle cx="12" cy="12" r="9" fill="none" stroke="var(--color-primary)" stroke-width="2" />
        <circle cx="12" cy="12" r="3.5" fill="var(--color-primary)" />
      </svg>
    {:else}
      <span class="mark" aria-hidden="true">{mark}</span>
    {/if}
    <p id={`${c.id}-who`}><b>{who}</b> <span class="time">{short(c.submitted)}</span></p>
    <span class="id">{c.id}{c.revision > 1 ? `, revision ${c.revision}` : ""}</span>
  </header>
  {#if c.why}<blockquote>{c.why}</blockquote>{/if}

  {#if open.length}
    <ol class="actions">
      {#each open as a (key(a))}
        <li>
          <div class="body">
            <p class="what"><b>{verb(a)}</b> {target(a)}</p>
            {#if detail(a)}<p class="because">{detail(a)}</p>{/if}
            {#if a.why}<p class="because">{a.why}</p>{/if}
            {#each a.flags as flag (flag)}<p class="flag">{FLAGS[flag] ?? flag}</p>{/each}
            {@render lines(a)}
          </div>
        </li>
      {/each}
    </ol>
    <p class="decide">Iris makes none of these changes before you approve them. Approve and deny come to this screen in the next update.</p>
  {/if}
  {#if problem}<p role="alert">{problem}</p>{/if}

  {#if decided.length}
    <details>
      <summary>{decided.length === 1 ? "One more change has a decision" : `${decided.length} more changes have a decision`}</summary>
      <ul class="decided">
        {#each decided as a (key(a))}
          <li><span><b>{verb(a)}</b> {target(a)}</span> <span class="outcome">{outcome(a)}</span></li>
        {/each}
      </ul>
    </details>
  {/if}
</article>

<style>
  .request {
    margin-bottom: var(--space-12);
    animation: arrive var(--transition-normal) both;
  }
  @keyframes arrive {
    from {
      opacity: 0;
      transform: translateY(var(--space-1));
    }
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--space-3);
  }
  header p {
    flex: 1;
    margin: 0;
  }
  .mark {
    flex: none;
    display: grid;
    place-items: center;
    width: var(--space-8);
    height: var(--space-8);
    border-radius: var(--radius-full);
    border: 1px solid var(--color-primary);
    color: var(--color-primary);
    font-size: var(--font-size--1);
    font-weight: 600;
  }
  svg.mark {
    border: 0;
  }
  .time,
  .id {
    color: var(--color-text-faint);
    font-size: var(--font-size--1);
  }
  .time {
    margin-left: var(--space-1);
  }
  blockquote {
    margin: var(--space-4) 0 var(--space-6);
    padding-left: var(--space-4);
    border-left: 2px solid var(--color-primary);
    font-size: var(--font-size-1);
    line-height: 1.4;
    text-wrap: pretty;
  }
  ol,
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .actions > li {
    display: flex;
    gap: var(--space-3);
    padding: var(--space-4) 0;
    border-top: 1px solid var(--color-border-subtle);
  }
  .body {
    flex: 1;
    min-width: 0;
  }
  .what {
    margin: 0;
  }
  .what b {
    font-weight: 600;
  }
  .because {
    margin: var(--space-1) 0 0;
    color: var(--color-text-muted);
  }
  .flag {
    margin: var(--space-2) 0 0;
    font-size: var(--font-size--1);
    color: var(--color-text-muted);
  }
  .flag::before {
    content: "";
    display: inline-block;
    width: var(--space-2);
    height: var(--space-2);
    margin: 0 var(--space-2) 1px 0;
    border-radius: var(--radius-full);
    background: var(--color-warning);
  }
  .messages {
    margin-top: var(--space-3);
    border-radius: var(--radius-md);
    background: var(--color-background-elevated);
    border: 1px solid var(--color-border-subtle);
  }
  .messages li + li {
    border-top: 1px solid var(--color-border-subtle);
  }
  .messages a,
  .gone {
    display: grid;
    grid-template-columns: minmax(0, 10rem) minmax(0, 1fr) auto;
    gap: var(--space-3);
    padding: var(--space-2) var(--space-3);
    font-size: var(--font-size--1);
    text-decoration: none;
  }
  .messages a:hover {
    background: var(--color-background-surface);
  }
  .from,
  .subject {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .from {
    font-weight: 500;
  }
  .subject {
    color: var(--color-text-muted);
  }
  .date,
  .gone {
    color: var(--color-text-faint);
  }
  .gone {
    display: block;
  }
  button {
    font: inherit;
    cursor: pointer;
  }
  .more {
    margin-top: var(--space-2);
    padding: 0;
    border: 0;
    background: none;
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
    text-decoration: underline;
    text-underline-offset: 3px;
  }
  .more:hover {
    color: var(--color-text);
  }
  .decide {
    margin: 0;
    padding-top: var(--space-4);
    border-top: 1px solid var(--color-border-subtle);
    color: var(--color-text-faint);
    font-size: var(--font-size--1);
  }
  details {
    margin-top: var(--space-6);
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  summary {
    cursor: pointer;
    width: max-content;
  }
  .decided li {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    gap: var(--space-1) var(--space-4);
    padding: var(--space-2) 0;
    border-bottom: 1px solid var(--color-border-subtle);
  }
  .outcome {
    color: var(--color-text-faint);
  }
  @media (max-width: 600px) {
    .messages a {
      grid-template-columns: minmax(0, 1fr) auto;
    }
    .messages a {
      row-gap: 0;
    }
    .subject {
      grid-column: 1 / -1;
      grid-row: 2;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .request,
    .actions > li {
      animation: none;
      transition: none;
    }
  }
</style>
