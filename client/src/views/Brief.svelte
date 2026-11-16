<!--
  The brief (spec 1 section 8): the state of the mail, not a message list.
  It opens with what needs you, in words. Each section shows at most 7 lines
  and links its full list. Read only in release 1.
-->
<script lang="ts">
  import { get, type Brief, type Item, type Status } from "../api";
  import { poll as every, watch } from "../clock";
  import { greeting, liveCode, party, partyKey, summary, when } from "../format";
  import Code from "./Code.svelte";
  import Line from "./Line.svelte";

  let {
    brief: firstBrief,
    status: firstStatus = null,
    now: start = new Date(),
    poll = 5000,
  }: { brief: Brief; status?: Status | null; now?: Date; poll?: number } = $props();

  // svelte-ignore state_referenced_locally
  let brief = $state(firstBrief);
  // svelte-ignore state_referenced_locally
  let status = $state(firstStatus);
  // svelte-ignore state_referenced_locally
  let now = $state(start);
  const ready = (s: Status | null) => !s || s.accounts.some((a) => a.mirror_ready_at);

  // Spec 1 section 4.2: the brief grows while the first sync runs. Load it again until the mirror is ready.
  $effect(() => {
    if (!poll || ready(status)) return;
    return every(
      () => Promise.all([get<Brief>("brief"), get<Status>("status")]),
      ([b, s]) => ((brief = b), (status = s)),
      poll,
      ([, s]) => ready(s),
    );
  });

  const SHOWN = 7;

  let codes = $derived(brief.signals.items.filter((i) => liveCode(i, now)));
  // At the expiry of the first live code, the code moves from the top to Signals.
  let nextExpiry = $derived(codes.map((i) => i.expires!).sort()[0]);
  $effect(() => (nextExpiry ? watch(nextExpiry, (d) => (now = d)) : undefined));
  let shown = $derived(new Set(brief.conversations.items.map((i) => i.id)));
  let proposal = $derived(brief.proposal && brief.proposal.count > 0 ? brief.proposal : null);
  let parts = $derived([
    { key: "you_owe", title: "You owe", ...brief.you_owe },
    { key: "owed_to_you", title: "Owed to you", ...brief.owed_to_you },
    { key: "conversations", title: "Conversations", ...brief.conversations },
    // A conversation with a stranger is in both sections of the API. It shows once, under Conversations.
    {
      key: "strangers",
      title: "From senders you do not know",
      ...brief.strangers,
      count: brief.strangers.count - brief.strangers.items.filter((i) => shown.has(i.id)).length,
      items: brief.strangers.items.filter((i) => !shown.has(i.id)),
    },
    {
      key: "signals",
      title: "Signals",
      ...brief.signals,
      count: brief.signals.count - codes.length,
      items: brief.signals.items.filter((i) => !codes.includes(i)),
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
  let records = $derived(Object.entries(brief.records).filter(([, n]) => n > 0));
  let copying = $derived(status && !status.accounts.some((a) => a.mirror_ready_at) ? status.messages : null);
  let day = $derived(now.toLocaleDateString("en-US", { weekday: "long", month: "long", day: "numeric" }));
</script>

<div class="opening">
  <p class="day">{greeting(now)} It is {day}.</p>
  <h1>{summary(brief)}</h1>
</div>

{#if copying}
  <div class="copying" role="status">
    <p>
      Iris still copies your mailbox: {copying.fetched.toLocaleString("en-US")} of {copying.total ? copying.total.toLocaleString("en-US") : "all"} messages.
      {brief.oldest_fetched ? `This brief covers the mail since ${when(brief.oldest_fetched)}.` : "The brief grows as the copy continues."}
    </p>
    <progress max={copying.total || 1} value={copying.fetched} aria-label="Messages copied"></progress>
  </div>
{/if}

{#if codes.length}
  <div class="codes">
    {#each codes as item (item.id)}<Code {item} {now} />{/each}
  </div>
{/if}

{#each parts as part (part.key)}
  {#if part.count > 0}
    <section aria-labelledby={part.key}>
      <h2 id={part.key}><span>{part.title}</span> <span class="count">{part.count}</span></h2>
      <ul>
        {#each part.items.slice(0, SHOWN) as item (item.id)}<Line {item} {now} twins={part.duplicates?.[item.id]} />{/each}
      </ul>
      {#if part.count > SHOWN}<a class="all" href={`#/brief/${part.key}`}>All {part.count}</a>{/if}
    </section>
  {/if}
{/each}

{#if proposal}
  <section class="proposal" aria-labelledby="proposal">
    <h2 id="proposal"><span>Triage proposal</span> <span class="count">{proposal.count}</span></h2>
    <p>
      Iris would make {proposal.count.toLocaleString("en-US")} changes to your mailbox:
      {Object.entries(proposal.verbs).map(([verb, n]) => `${verb} ${n.toLocaleString("en-US")}`).join(", ")}. Nothing is applied.
    </p>
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
  <p>Iris only observes. It changed nothing in your mailbox.</p>
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
  .proposal p {
    margin: 0;
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
