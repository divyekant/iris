<!--
  One changeset in the review (R2-06, spec 2 section 5.5): who asks, why, and
  each action with its real targets (messages or items) and the arguments
  that set its effect. Each waiting change starts ticked. The person unticks
  what they do not want, and one button approves the ticked changes and
  denies the others. The person can also withdraw the whole request, after
  one question in the page.
-->
<script lang="ts">
  import { get, post, type Changeset, type Item, type Target } from "../api";
  import { author, detail, FLAGS, itemRow, key, messageRow, outcome, pages, plan, short, shown, target, targets, verb, waits, type Row } from "../review";

  // `onchange` gets the changeset after a decision, so that the review shows it as it is now.
  let { changeset: c, onchange = () => {} }: { changeset: Changeset; onchange?: (c: Changeset) => void } = $props();

  // The messages endpoint takes at most 200 IDs, and the engine at most 500 decisions (issue 197).
  const PAGE = 200;
  const BATCH = 500;

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
  // The changes whose last load failed. They load again only with "Try again".
  let failed: Record<string, boolean> = $state({});
  let problem = $state("");
  const complete = (a: (typeof actions)[number]) => (rows[key(a)]?.length ?? -1) >= new Set(targets(a)).size;
  function load(a: (typeof actions)[number]) {
    loader.more(a, PAGE).then(
      (list) => {
        if (list) rows[key(a)] = list;
      },
      (e) => {
        failed[key(a)] = true;
        problem = e instanceof Error ? e.message : String(e);
      },
    );
  }
  // Spec 2 section 5.5: the review shows each action fully resolved. Each page of rows that
  // arrives starts the next one, until each waiting change has all of its rows.
  $effect(() => {
    for (const a of open) if (!complete(a) && !failed[key(a)]) load(a);
  });
  // The person decides only when the screen has every row of each waiting change.
  let missing = $derived(open.filter((a) => !complete(a)));
  function retry() {
    problem = "";
    failed = {};
  }

  let ready = $derived(!busy && !stale && !missing.length);

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
  // Spec 2 section 5.5: the person can withdraw an open request. The store cancels each change
  // that Iris did not start; a change that Iris started goes on to its end. The page cannot see
  // which approved changes Iris started, so the question promises no count, and the result
  // counts from the states that the store gives back. A withdraw cannot be undone: the page asks once.
  let asking = $state(false);
  async function withdraw() {
    if (busy || stale) return;
    busy = true;
    problem = saved = "";
    const before = new Set(c.actions.filter((a) => a.decision?.state === "cancelled").map(key));
    try {
      const after = await post<Changeset>(`changesets/${encodeURIComponent(c.id)}/withdraw`, {});
      onchange(after);
      asking = false;
      const gone = after.actions.filter((a) => a.decision?.state === "cancelled" && !before.has(key(a))).length;
      const going = after.actions.filter((a) => a.decision?.state === "approved").length;
      saved = [
        `You withdrew this request. Iris cancelled ${gone === 1 ? "one change" : `${gone} changes`}.`,
        going ? `Iris started ${going === 1 ? "one change" : `${going} changes`} before the withdraw, and ${going === 1 ? "it goes" : "they go"} on to the end.` : "",
      ].join(" ").trim();
    } catch (e) {
      const status = (e as { status?: number }).status;
      asking = false;
      if (status === 409) await refresh("Iris did not withdraw the request, because it changed after you opened it. Here it is as it is now.");
      else if (status === undefined || status >= 500) await refresh("Iris does not know if it withdrew the request. Here it is as it is now.");
      else problem = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  async function decide() {
    const yes = chosen.length;
    const no = open.length - yes;
    if (!ready) return;
    busy = true;
    problem = saved = "";
    const all = open.map((a) => ({ revision: a.revision, n: a.n, approve: !off[key(a)], hash: a.hash }));
    // More than 500 changes go in batches of 500. Each batch is one transaction of the engine.
    let done = 0;
    try {
      for (; done < all.length; done += BATCH) {
        onchange(await post<Changeset>(`changesets/${encodeURIComponent(c.id)}/decide`, { actions: all.slice(done, done + BATCH) }));
      }
      saved = [yes && `You approved ${yes === 1 ? "one change" : `${yes} changes`}.`, no && `You denied ${no === 1 ? "one change" : `${no} changes`}.`].filter(Boolean).join(" ");
    } catch (e) {
      const status = (e as { status?: number }).status;
      const before = done ? `Iris saved your decision on the first ${done} changes. ` : "";
      // A 409 is a definite refusal: the batch wrote nothing. With no answer or a 5xx, the result is unknown.
      if (status === 409) await refresh(`${before}Iris did not save ${done ? "the rest" : "this decision"}, because the request changed after you opened it. Here it is as it is now.`);
      else if (status === undefined || status >= 500) await refresh(`${before}Iris does not know if it saved ${done ? "the rest" : "your decision"}. Here is the request as it is now.`);
      else problem = `${before}${e instanceof Error ? e.message : String(e)}`;
    } finally {
      busy = false;
    }
  }
</script>

{#snippet lines(a: (typeof actions)[number])}
  {@const list = rows[key(a)] ?? []}
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
{/snippet}

{#snippet question()}
  <div class="question" role="group" aria-labelledby={`${c.id}-ask`}>
    <p id={`${c.id}-ask`}><b>Withdraw this request?</b></p>
    <p>
      Iris cancels each change of this request that it did not start. A change that Iris made or started stays in your
      mailbox, also the part of a failed change that Iris made before it failed. You cannot undo a withdraw.
    </p>
    <div class="answers">
      <button type="button" class="outline" disabled={busy} onclick={withdraw}>Withdraw the request</button>
      <button type="button" class="quiet" disabled={busy} onclick={() => (asking = false)}>Keep it</button>
    </div>
  </div>
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
    {#if c.state === "open" && !asking}
      <button type="button" class="quiet" disabled={busy || stale} onclick={() => (asking = true)}>Withdraw</button>
    {/if}
  </header>
  {#if asking}{@render question()}{/if}
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
      <p>Iris makes none of these changes before you approve them.</p>
      {#if missing.length && !problem}<p>Iris loads everything that these changes touch, so that you see it before you decide.</p>{/if}
    </div>
  {/if}
  {#if c.state === "withdrawn" && !saved}<p class="saved">This request is withdrawn.</p>{/if}
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
  /* Each row of a change shows. After about six rows, the box scrolls. */
  .messages {
    max-height: calc(var(--space-12) * 5);
    overflow-y: auto;
    overscroll-behavior: contain;
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
  .quiet {
    padding: 0;
    border: 0;
    background: none;
    color: var(--color-text-faint);
    font-size: var(--font-size--1);
    text-decoration: underline;
    text-decoration-color: transparent;
    text-underline-offset: 3px;
    transition: color var(--transition-fast), text-decoration-color var(--transition-fast);
  }
  .quiet:hover,
  .quiet:focus-visible {
    color: var(--color-text);
    text-decoration-color: currentColor;
  }
  .quiet:disabled {
    cursor: default;
    opacity: 0.6;
  }
  /* The withdraw question: a notice with a warning line, as the observe note of the review. */
  .question {
    margin: var(--space-4) 0 var(--space-6);
    padding-left: var(--space-4);
    border-left: 4px solid var(--color-warning);
    animation: arrive var(--transition-normal) both;
  }
  .question p {
    margin: 0 0 var(--space-2);
    max-width: 60ch;
    color: var(--color-text-muted);
  }
  .question p b {
    color: var(--color-text);
    font-weight: 600;
  }
  .answers {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-4);
    margin-top: var(--space-3);
  }
  .outline {
    padding: var(--space-2) var(--space-4);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-full);
    background: transparent;
    color: var(--color-text);
    font-weight: 500;
    transition: border-color var(--transition-fast);
  }
  .outline:hover,
  .outline:focus-visible {
    border-color: var(--color-text-muted);
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
