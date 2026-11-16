<!--
  The brief (spec 1 section 8): the state of the mail, not a message list.
  It opens with what needs you, in words. Each section shows at most 7 lines
  and links its full list. Read only in release 1.
-->
<script lang="ts">
  import { get, loadBrief, type Brief, type Item, type Status } from "../api";
  import { poll as every, watch } from "../clock";
  import { across, copying, dueSoon, greeting, nextDay, party, partyKey, signals, summary, unfinished, when } from "../format";
  import Code from "./Code.svelte";
  import Bundle from "./Bundle.svelte";
  import Line from "./Line.svelte";
  import Gate from "./Gate.svelte";

  let {
    brief: firstBrief,
    status: firstStatus = null,
    now: start = new Date(),
    poll = 5000,
  }: { brief: Brief; status?: Status | null; now?: Date; poll?: number } = $props();

  // The API puts a conversation with a stranger only under Strangers, with exact counts (dk, 2026-11-16).
  // svelte-ignore state_referenced_locally
  let brief = $state(firstBrief);
  // svelte-ignore state_referenced_locally
  let status = $state(firstStatus);
  // svelte-ignore state_referenced_locally
  let now = $state(start);
  const ready = (s: Status | null) => copying(s).length === 0;

  // Load the brief again while it is open: every 5 seconds while the first sync
  // runs (spec 1 section 4.2), then every minute for new mail.
  let syncing = $derived(!ready(status));
  $effect(() => {
    if (!poll) return;
    return every(
      () => Promise.all([loadBrief(), get<Status>("status")]),
      ([b, s]) => ((brief = b), (status = s)),
      syncing ? poll : poll * 12,
    );
  });

  const SHOWN = 7;
  // After an answer at the gate, the items of the sender move: load the brief again.
  // The gate stays after the last answer, so that its Undo stays too.
  let answered = $state(false);
  const reload = () => ((answered = true), void loadBrief().then((b) => (brief = b)).catch(() => {}));

  let signal = $derived(signals(brief.signals, now));
  let codes = $derived(signal.codes);
  // A signal leaves at its expiry. The date and the due words change at midnight.
  // Keep `now` fresh until the first of the two.
  let until = $derived([signal.next, nextDay(now)].filter((t): t is string => !!t).sort((a, b) => Date.parse(a) - Date.parse(b))[0]);
  $effect(() => watch(until, (d) => (now = d)));
  let proposal = $derived(brief.proposal && brief.proposal.count > 0 ? brief.proposal : null);
  // What you owe that is due in 2 days or less, or late, leads the brief (R3-08). It shows only there, so You owe counts the rest.
  // shortcut: the API gives at most 200 items of You owe, so a due item past them stays in You owe; page the API if one person owes more.
  let soon = $derived(dueSoon(brief.you_owe.items, now));
  let parts = $derived([
    { key: "you_owe", title: "You owe", count: brief.you_owe.count - soon.length, items: brief.you_owe.items.filter((i) => !soon.includes(i)), duplicates: brief.you_owe.duplicates, total: brief.you_owe.count },
    { key: "owed_to_you", title: "Owed to you", ...brief.owed_to_you },
    { key: "conversations", title: "Conversations", ...brief.conversations },
    { key: "strangers", title: "From senders you do not know", ...brief.strangers },
    {
      key: "signals",
      title: "Signals",
      ...brief.signals,
      // The count of the API, less the signals that expired since. The live codes show at the top, so only the rows leave them out.
      count: signal.count,
      items: signal.rows,
    },
  ]);

  // The feed shows by publisher. Two senders with the same name stay apart.
  let publishers = $derived.by(() => {
    const groups = new Map<string, { name: string; items: Item[] }>();
    for (const item of brief.feed.items) {
      const key = partyKey(item.publisher);
      const group = groups.get(key) ?? { name: party(item.publisher), items: [] };
      group.items.push(item);
      groups.set(key, group);
    }
    return [...groups].sort((a, b) => b[1].items.length - a[1].items.length);
  });
  // Counts for each sender are exact only when the API gave every feed item.
  let feedComplete = $derived(brief.feed.items.length >= brief.feed.count);
  let bundles = $derived(brief.bundles ?? []);
  let hidden = $derived(brief.hidden ?? 0);
  // An API from before R2-09 gives only the hidden count.
  let handled = $derived(brief.handled ?? { count: hidden, verbs: {} });
  // The messages of the actions that a rule approved. An approved action can still wait to apply, so the brief says decided, not changed.
  let decided = $derived(Object.values(handled.verbs).reduce((a, b) => a + b, 0));
  let records = $derived(Object.entries(brief.records).filter(([, n]) => n > 0));
  let progress = $derived(status && unfinished(status).length ? status.messages : null);
  let stopped = $derived(copying(status).length === 0);
  let day = $derived(now.toLocaleDateString("en-US", { weekday: "long", month: "long", day: "numeric" }));
</script>

<div class="opening">
  <p class="day">{greeting(now)} It is {day}.</p>
  <h1>{summary(brief)}</h1>
</div>


{#if progress}
  <div class="copying" role="status">
    <p>
      {#if stopped}
        Iris copied {progress.fetched.toLocaleString("en-US")} of {progress.total ? progress.total.toLocaleString("en-US") : "all"} messages{across(status)}, then Google stopped the sign-in.
      {:else}
        Iris still copies your mailbox: {progress.fetched.toLocaleString("en-US")} of {progress.total ? progress.total.toLocaleString("en-US") : "all"} messages{across(status)}.
      {/if}
      {brief.oldest_fetched ? `This brief covers the mail since ${when(brief.oldest_fetched)}.` : stopped ? "This brief covers the mail that Iris copied." : "The brief grows as the copy continues."}
    </p>
    <progress max={progress.total || 1} value={progress.fetched} aria-label="Messages copied"></progress>
  </div>
{/if}

{#if codes.length}
  <div class="codes">
    {#each codes as item (item.id)}<Code {item} {now} />{/each}
  </div>
{/if}

{#if soon.length}
  <section class="soon" aria-labelledby="soon">
    <h2 id="soon"><span>Due soon</span> <span class="count">{soon.length}</span></h2>
    <ul>
      {#each soon.slice(0, SHOWN) as item (item.id)}<Line {item} {now} twins={brief.you_owe.duplicates?.[item.id]} />{/each}
    </ul>
    {#if soon.length > SHOWN}<a class="all" href="#/brief/you_owe">All {brief.you_owe.count} that you owe</a>{/if}
  </section>
{/if}

{#if brief.review?.actions}
  <section aria-labelledby="review">
    <h2 id="review"><span>Waits for your review</span> <span class="count">{brief.review.actions}</span></h2>
    {#if proposal}
      <p class="note">Triage proposes to {Object.entries(proposal.verbs).map(([verb, n]) => `${verb} ${n.toLocaleString("en-US")} ${n === 1 ? "message" : "messages"}`).join(", ")}.</p>
    {/if}
    <a class="all" href="#/review">Open the review</a>
  </section>
{/if}

{#each parts as part (part.key)}
  {#if part.key === "strangers" && brief.gate}
    {#if brief.gate.count > 0 || (brief.declined ?? 0) > 0 || answered}
      <section aria-labelledby="strangers">
        <h2 id="strangers"><span>{part.title}</span> <span class="count">{brief.gate.count}</span></h2>
        <Gate page={brief.gate} declined={brief.declined} onanswer={reload} />
      </section>
    {/if}
  {:else if part.count > 0}
    <section aria-labelledby={part.key}>
      <h2 id={part.key}><span>{part.title}</span> <span class="count">{part.count}</span></h2>
      <!-- Live login codes show above the sections, so the Signals list can be empty. -->
      {#if part.items.length}
        <ul>
          {#each part.items.slice(0, SHOWN) as item (item.id)}<Line {item} {now} twins={part.duplicates?.[item.id]} />{/each}
        </ul>
      {/if}
      <a class="all" href={`#/brief/${part.key}`}>All {part.total ?? part.count}</a>
    </section>
  {/if}
{/each}

{#if bundles.length}
  <section aria-labelledby="bundles">
    <h2 id="bundles"><span>Bundled by your rules</span> <span class="count">{bundles.reduce((n, b) => n + b.count, 0)}</span></h2>
    <ul>
      {#each bundles as bundle (bundle.rule)}<Bundle {bundle} />{/each}
    </ul>
  </section>
{/if}

{#if handled.count > 0}
  <section aria-labelledby="handled">
    <h2 id="handled"><span>Handled without you</span> <span class="count">{handled.count}</span></h2>
    <p class="note">{[hidden ? `${hidden.toLocaleString("en-US")} hidden from this brief` : "", decided ? `${decided.toLocaleString("en-US")} ${decided === 1 ? "message" : "messages"} that your rules decided` : ""].filter(Boolean).join(", ")}.</p>
    <a class="all" href="#/handled">See what and why</a>
  </section>
{/if}

{#if publishers.length || records.length}
  <section class="also" aria-labelledby="also">
    <h2 id="also">Also in your mail</h2>
    {#if publishers.length}
      <p class="label">To read when you choose</p>
      <ul class="chips">
        {#each publishers.slice(0, SHOWN) as [key, group] (key)}
          <li><a href="#/brief/feed">{group.name}{#if feedComplete} <span>{group.items.length}</span>{/if}</a></li>
        {/each}
        {#if publishers.length > SHOWN || !feedComplete}<li><a href="#/brief/feed">All {brief.feed.count}</a></li>{/if}
      </ul>
    {/if}
    {#if records.length}
      <p class="label">Records</p>
      <ul class="chips types">
        {#each records as [type, n] (type)}<li><a href={`#/records/${type}`}>{type} <span>{n}</span></a></li>{/each}
      </ul>
    {/if}
  </section>
{/if}

<footer>
  {#if brief.hostile > 0}
    <p class="warning">Iris found {brief.hostile} hostile messages in the last 7 days. The brief does not show their subject or their sender.</p>
  {/if}
  {#if brief.not_understood > 0}
    <p>Iris could not read {brief.not_understood} messages. They stay in your mailbox as they are.</p>
  {/if}
  {#if (status?.mode ?? "observe") === "observe" && decided === 0}<p>Iris only observes. It changed nothing in your mailbox.</p>{/if}
</footer>

<style>
  .opening {
    margin: var(--space-8) 0 var(--space-12);
    animation: arrive var(--transition-normal) both;
  }
  .day {
    color: var(--color-text-muted);
    margin: 0 0 var(--space-3);
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
  @keyframes arrive {
    from {
      opacity: 0;
      transform: translateY(var(--space-1));
    }
  }
  .copying {
    margin-bottom: var(--space-8);
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  .copying p {
    margin: 0 0 var(--space-2);
  }
  progress {
    width: 100%;
    height: var(--space-1);
    accent-color: var(--color-primary);
  }
  .codes {
    display: grid;
    gap: var(--space-3);
    margin-bottom: var(--space-12);
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
    margin: 0;
    padding: 0;
  }
  .all {
    display: inline-block;
    margin-top: var(--space-2);
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  .all:hover {
    color: var(--color-text);
  }
  .note {
    margin: var(--space-2) 0 0;
    color: var(--color-text-muted);
  }
  .label {
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
    margin: var(--space-4) 0 var(--space-2);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
  }
  .chips a {
    display: inline-block;
    padding: var(--space-1) var(--space-3);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-full);
    background: var(--color-background-elevated);
    color: var(--color-text);
    text-decoration: none;
    transition: border-color var(--transition-fast);
  }
  .types a {
    text-transform: capitalize;
  }
  .chips a:hover {
    border-color: var(--color-primary);
  }
  .chips span {
    color: var(--color-text-muted);
    margin-left: var(--space-1);
  }
  footer {
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
    border-top: 1px solid var(--color-border-subtle);
    padding-top: var(--space-4);
  }
  footer p {
    margin: 0 0 var(--space-2);
  }
  .soon {
    border-left: 4px solid var(--color-warning);
    padding-left: var(--space-3);
  }
  .warning {
    color: var(--color-text);
    border-left: 4px solid var(--color-warning);
    padding-left: var(--space-3);
  }
  @media (max-width: 600px) {
    h1 {
      font-size: var(--font-size-3);
    }
    .opening {
      margin: var(--space-4) 0 var(--space-8);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .opening {
      animation: none;
    }
    .chips a {
      transition: none;
    }
  }
</style>
