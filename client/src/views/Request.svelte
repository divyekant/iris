<!--
  One changeset in the review (R2-06, spec 2 section 5.5): who asks, why, and
  each action with its real targets (messages or items) and the arguments
  that set its effect. Each waiting change starts ticked. The person unticks
  what they do not want, and one button approves the ticked changes and
  denies the others.
-->
<script lang="ts">
  import { get, post, type Changeset, type Item, type Target } from "../api";
  import { author, detail, FLAGS, itemRow, key, messageRow, outcome, pages, plan, short, shown, target, targets, verb, waits, type Row } from "../review";

  // `onchange` gets the changeset after a decision, so that the review shows it as it is now.
  let { changeset: c, onchange = () => {} }: { changeset: Changeset; onchange?: (c: Changeset) => void } = $props();

  const FIRST = 3;
  const PAGE = 200;

  let actions = $derived(shown(c));
  let open = $derived(actions.filter(waits));
  let decided = $derived(actions.filter((a) => !waits(a)));
  // The unticked changes. A change that is not in this list is ticked, also one that comes with a new revision.
  let off: Record<string, boolean> = $state({});
  let chosen = $derived(open.filter((a) => !off[key(a)]));
  // "more" only beside changes that wait.
  let more = $derived(open.length ? "more " : "");
  let busy = $state(false);
  let saved = $state("");
  // True while the screen may not show the request as the store has it: after a decision
  // with an unknown result, or a 409, until the request loads again.
  let stale = $state(false);
  let who = $derived(author(c));
  let mark = $derived(who.split(/\s+/).slice(0, 2).map((w) => w[0]?.toUpperCase()).join(""));

  // The targets of each action, loaded in pages: messages by their summaries, items one by one.
  const loader = pages(async (ids, items) => {
    if (!items) return (await get<{ messages: Target[] }>(`messages?ids=${ids.map(encodeURIComponent).join(",")}`)).messages.map(messageRow);
    return Promise.all(
      ids.map((id) =>
        get<Item>(`items/${encodeURIComponent(id)}`).then(
          (i) => itemRow(id, i),
          // Only a 404 says that the item is gone. Each other failure is a problem to show.
          (e) => {
            if ((e as { status?: number }).status === 404) return itemRow(id, null);
            throw e;
          },
        ),
      ),
    );
  });
  let rows: Record<string, Row[]> = $state({});
  let problem = $state("");
  function load(a: (typeof actions)[number], count: number) {
    loader.more(a, count).then(
      (list) => {
        if (list) rows[key(a)] = list;
      },
      (e) => (problem = e instanceof Error ? e.message : String(e)),
    );
  }
  $effect(() => {
    for (const a of actions) if (!(key(a) in rows)) load(a, FIRST);
  });
  // The person decides only on what the screen shows: the first rows of each waiting change.
  let missing = $derived(open.filter((a) => !(key(a) in rows)));
  function retry() {
    problem = "";
    for (const a of missing) load(a, FIRST);
  }

  // The engine takes at most this many decisions in one request (issue 197).
  const MAX = 500;
  let blocked = $derived(
    open.length > MAX
      ? `This request has ${open.length.toLocaleString("en-US")} changes. Iris can decide at most ${MAX} in one step, so it cannot decide this request yet.`
      : "",
  );
  let ready = $derived(!busy && !stale && !missing.length && !blocked);

  /** Load the request again, so that the screen shows it as the store has it. */
  async function refresh(why: string) {
    stale = true;
    try {
      onchange(await get<Changeset>(`changesets/${encodeURIComponent(c.id)}`));
      stale = false;
      problem = why;
    } catch {
      problem = `${why} Iris could not load the request again. Load it again before you decide.`.trim();
    }
  }

  // One request for each decision: the engine approves the ticked changes, with the hash that
  // this screen showed, and denies the others. If one is refused, it writes nothing (409).
  async function decide() {
    const yes = chosen.length;
    const no = open.length - yes;
    if (!ready) return;
    busy = true;
    problem = saved = "";
    try {
      const body = { actions: open.map((a) => ({ revision: a.revision, n: a.n, approve: !off[key(a)], hash: a.hash })) };
      onchange(await post<Changeset>(`changesets/${encodeURIComponent(c.id)}/decide`, body));
      saved = [yes && `You approved ${yes === 1 ? "one change" : `${yes} changes`}.`, no && `You denied ${no === 1 ? "one change" : `${no} changes`}.`].filter(Boolean).join(" ");
    } catch (e) {
      const status = (e as { status?: number }).status;
      // A 409 is a definite refusal: the transaction wrote nothing. With no answer or a 5xx, the result is unknown.
      if (status === 409) await refresh("Iris did not save this decision, because the request changed after you opened it. Here it is as it is now.");
      else if (status === undefined || status >= 500) await refresh("Iris does not know if it saved your decision. Here is the request as it is now.");
      else problem = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }
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
        {@const id = `${c.id}-${key(a)}`}
        <li class:off={off[key(a)]}>
          <input type="checkbox" {id} checked={!off[key(a)]} disabled={busy} onchange={(e) => (off[key(a)] = !e.currentTarget.checked)} />
          <div class="body">
            <label class="what" for={id}><b>{verb(a)}</b> {target(a)}</label>
            {#if detail(a)}<p class="because">{detail(a)}</p>{/if}
            {#if a.why}<p class="because">{a.why}</p>{/if}
            {#each a.flags as flag (flag)}<p class="flag">{FLAGS[flag] ?? flag}</p>{/each}
            {#if off[key(a)]}<p class="denied">Iris will deny this change.</p>{/if}
            {@render lines(a)}
          </div>
        </li>
      {/each}
    </ol>
    <div class="decide">
      <button type="button" class="primary" class:deny={!chosen.length} disabled={!ready} onclick={decide}>{plan(chosen.length, open.length)}</button>
      <p>{blocked || "Iris makes none of these changes before you approve them."}</p>
    </div>
  {/if}
  {#if problem}
    <p role="alert">
      {problem}
      {#if stale}<button type="button" class="more" disabled={busy} onclick={() => refresh("")}>Load it again</button>
      {:else if missing.length}<button type="button" class="more" onclick={retry}>Try again</button>{/if}
    </p>
  {/if}
  {#if saved}<p class="saved" role="status">{saved}</p>{/if}

  {#if decided.length}
    <details open={!open.length}>
      <summary>{decided.length === 1 ? `One ${more}change has a decision` : `${decided.length} ${more}changes have a decision`}</summary>
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
  .actions > li.off .body > :not(.denied) {
    opacity: 0.5;
    transition: opacity var(--transition-normal);
  }
  input[type="checkbox"] {
    appearance: none;
    flex: none;
    position: relative;
    width: var(--space-6);
    height: var(--space-6);
    margin: 0;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-full);
    background: var(--color-background-elevated);
    cursor: pointer;
    transition: background var(--transition-fast), border-color var(--transition-fast);
  }
  input[type="checkbox"]:checked {
    background: var(--color-primary);
    border-color: var(--color-primary);
  }
  input[type="checkbox"]:focus-visible,
  .primary:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }
  /* A tick drawn with two borders, in the color of text on primary. */
  input[type="checkbox"]:checked::after {
    content: "";
    position: absolute;
    left: 8px;
    top: 4px;
    width: 5px;
    height: 10px;
    border: solid var(--color-on-primary);
    border-width: 0 2px 2px 0;
    transform: rotate(45deg);
  }
  .body {
    flex: 1;
    min-width: 0;
  }
  .what {
    display: block;
    margin: 0;
    cursor: pointer;
    line-height: var(--space-6);
  }
  .what b {
    font-weight: 600;
  }
  .because,
  .denied {
    margin: var(--space-1) 0 0;
    color: var(--color-text-muted);
  }
  .denied {
    font-size: var(--font-size--1);
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
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3) var(--space-4);
    padding-top: var(--space-4);
    border-top: 1px solid var(--color-border-subtle);
  }
  .decide p {
    margin: 0;
    color: var(--color-text-faint);
    font-size: var(--font-size--1);
  }
  .primary {
    padding: var(--space-2) var(--space-6);
    border: 1px solid var(--color-primary);
    border-radius: var(--radius-full);
    background: var(--color-primary);
    color: var(--color-on-primary);
    font-weight: 600;
    transition: background var(--transition-fast), color var(--transition-fast);
  }
  .primary.deny {
    background: transparent;
    color: var(--color-text);
    border-color: var(--color-border);
  }
  .primary:disabled {
    cursor: progress;
    opacity: 0.6;
  }
  .saved {
    margin: var(--space-4) 0 0;
    color: var(--color-text-muted);
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
    .actions > li,
    .actions > li.off .body > :not(.denied),
    input[type="checkbox"],
    .primary {
      animation: none;
      transition: none;
    }
  }
</style>
