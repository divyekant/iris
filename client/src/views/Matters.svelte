<!--
  Matters (card R6-01): mail from more than one thread that belongs
  together. The proposals of Iris come first, then the matters that you
  kept. Each line opens the matter.
-->
<script lang="ts">
  import type { Matter, MatterList } from "../api";
  import { matterCounts } from "../format";
  import { href } from "../route";

  let { list }: { list: MatterList } = $props();
  let total = (m: Matter) => m.counts.commitment + m.counts.conversation + m.counts.record;
</script>

{#snippet line(m: Matter)}
  <li>
    <a class="matter" href={href.matter(m.id)}>
      <span class="stack" aria-hidden="true"><span></span><span>{total(m)}</span></span>
      <span class="what">
        <span class="title">{m.title}</span>
        <span class="detail">{matterCounts(m.counts)}</span>
      </span>
      <span class="chevron" aria-hidden="true">›</span>
    </a>
  </li>
{/snippet}

<h1>Matters</h1>
<p class="lead">Iris groups the mail that belongs together, for example a trip or a lease. You decide.</p>

{#if list.proposed.length === 0 && list.confirmed.length === 0}
  <p class="empty">No matters yet. When mail from two or more threads belongs together, Iris proposes a matter here.</p>
{/if}
{#if list.proposed.length}
  <section aria-labelledby="proposed">
    <h2 id="proposed">Iris proposes</h2>
    <ul>{#each list.proposed as m (m.id)}{@render line(m)}{/each}</ul>
  </section>
{/if}
{#if list.confirmed.length}
  <section aria-labelledby="confirmed">
    <h2 id="confirmed">Yours</h2>
    <ul>{#each list.confirmed as m (m.id)}{@render line(m)}{/each}</ul>
  </section>
{/if}

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
  section {
    margin-bottom: var(--space-12);
  }
  h2 {
    font-size: var(--font-size--1);
    font-weight: 500;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--color-text-faint);
    margin: 0 0 var(--space-2);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .matter {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) 0;
    border-bottom: 1px solid var(--color-border-subtle);
    text-decoration: none;
  }
  .matter:hover .title {
    text-decoration: underline;
  }
  /* Two rounded squares, one on top of the other: a group of items, with the count. */
  .stack {
    position: relative;
    flex: none;
    width: 36px;
    height: 32px;
  }
  .stack span {
    position: absolute;
    width: 28px;
    height: 28px;
    box-sizing: border-box;
    border-radius: var(--radius-md);
    background: var(--color-background-surface);
    border: 1px solid var(--color-border);
  }
  .stack span:first-child {
    left: 8px;
    top: 0;
    opacity: 0.6;
  }
  .stack span:last-child {
    left: 0;
    top: 4px;
    display: grid;
    place-items: center;
    font-size: var(--font-size--1);
    font-weight: 600;
    color: var(--color-text-muted);
    font-variant-numeric: tabular-nums;
  }
  .what {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
  }
  .title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .detail {
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  .chevron {
    flex: none;
    color: var(--color-text-faint);
  }
</style>
