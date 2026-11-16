<!-- The brief (spec 1 section 8): the state of the mail, not a message list. Read only in release 1. -->
<script lang="ts">
  import type { Brief, Item, Section } from "../api";
  import { party } from "../format";
  import Line from "./Line.svelte";

  let { brief }: { brief: Brief } = $props();

  // The brief stays short. Each section links to its full list.
  const SHOWN = 7;

  type Part = { key: string; title: string; empty: string; section: Section; all?: string };
  let parts: Part[] = $derived([
    { key: "you_owe", title: "You owe", empty: "You owe nothing.", section: brief.you_owe, all: "#/commitments" },
    { key: "owed_to_you", title: "Owed to you", empty: "Nobody owes you anything.", section: brief.owed_to_you, all: "#/commitments" },
    { key: "conversations", title: "Conversations", empty: "No conversation waits for you.", section: brief.conversations, all: "#/conversations" },
    { key: "signals", title: "Signals", empty: "No active signals.", section: brief.signals, all: "#/signals" },
  ]);

  // The feed shows by publisher: one line for each publisher.
  let publishers = $derived.by(() => {
    const groups = new Map<string, Item[]>();
    for (const item of brief.feed.items) {
      const name = party(item.publisher);
      groups.set(name, [...(groups.get(name) ?? []), item]);
    }
    return [...groups].sort((a, b) => b[1].length - a[1].length);
  });
  let records = $derived(Object.entries(brief.records).filter(([, n]) => n > 0));
</script>

<h1>Brief</h1>

{#each parts as part (part.key)}
  <section aria-labelledby={part.key}>
    <h2 id={part.key}>{part.title} <span class="count">{part.section.count}</span></h2>
    {#if part.section.count === 0}
      <p class="empty">{part.empty}</p>
    {:else}
      <ul>
        {#each part.section.items.slice(0, SHOWN) as item (item.id)}<Line {item} />{/each}
      </ul>
      {#if part.section.count > SHOWN && part.all}<a class="more" href={part.all}>All {part.section.count}</a>{/if}
    {/if}
  </section>
{/each}

<section aria-labelledby="feed">
  <h2 id="feed">Feed <span class="count">{brief.feed.count}</span></h2>
  {#if publishers.length === 0}
    <p class="empty">Nothing new to read.</p>
  {:else}
    <ul class="plain">
      {#each publishers as [name, items] (name)}<li><a href="#/feed">{name}</a> <span class="count">{items.length} new</span></li>{/each}
    </ul>
  {/if}
</section>

<section aria-labelledby="records">
  <h2 id="records">Records</h2>
  {#if records.length === 0}
    <p class="empty">No records yet.</p>
  {:else}
    <ul class="plain">
      {#each records as [type, n] (type)}<li><a href="#/records">{type}</a> <span class="count">{n}</span></li>{/each}
    </ul>
  {/if}
</section>

{#if brief.strangers.count > 0}
  <section aria-labelledby="strangers">
    <h2 id="strangers">From strangers <span class="count">{brief.strangers.count}</span></h2>
    <p class="note">You have not written to these senders. Release 5 adds a gate for them.</p>
    <ul>
      {#each brief.strangers.items.slice(0, SHOWN) as item (item.id)}<Line {item} />{/each}
    </ul>
  </section>
{/if}

<footer>
  {#if brief.hostile > 0}
    <p class="warning">{brief.hostile} hostile messages in the last 7 days. Iris does not show their subject or sender.</p>
  {/if}
  {#if brief.not_understood > 0}
    <p class="note">Iris could not understand {brief.not_understood} messages.</p>
  {/if}
  <p class="note">Iris is in observe mode. It changed nothing in your mailbox.</p>
</footer>

<style>
  h1 {
    font-size: var(--font-size-3);
    margin: 0 0 var(--space-6);
  }
  section {
    margin-bottom: var(--space-8);
  }
  h2 {
    font-size: var(--font-size-1);
    margin: 0 0 var(--space-2);
  }
  .count {
    color: var(--color-text-muted);
    font-weight: 400;
    font-size: var(--font-size--1);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .plain li {
    padding: var(--space-1) 0;
  }
  .empty,
  .note {
    color: var(--color-text-muted);
    margin: var(--space-1) 0;
  }
  .more {
    display: inline-block;
    margin-top: var(--space-2);
    font-size: var(--font-size--1);
  }
  .warning {
    border-left: 4px solid var(--color-warning);
    padding-left: var(--space-3);
  }
  footer {
    border-top: 1px solid var(--color-border);
    padding-top: var(--space-4);
  }
</style>
