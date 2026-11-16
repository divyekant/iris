<!--
  One item on one line. The whole line opens the item. The envelope opens the
  source mail in one step. An unread dot marks a conversation that waits for you.
-->
<script lang="ts">
  import type { Item } from "../api";
  import { due, initials, money, origin, other, party, timeLeft, when } from "../format";
  import { href } from "../route";

  // `note`: one more muted line, for example the reason of a rule that hides the item.
  let { item, now = new Date(), twins = [], note = "" }: { item: Item; now?: Date; twins?: string[]; note?: string } = $props();

  let who = $derived(other(item));
  let date = $derived(item.form === "commitment" ? due(item, now) : null);
  let detail = $derived.by(() => {
    switch (item.form) {
      case "commitment":
        return [party(who), money(item.amount)].filter(Boolean).join(", ");
      case "conversation":
        return item.latest ? `${party(item.latest.from)}: ${item.latest.snippet}` : party(who);
      case "feed":
        return `${party(who)}, ${when(item.published)}`;
      case "signal": {
        const left = timeLeft(item.expires, now);
        return `${party(who)}, ${left ? `ends ${left}` : "ended"}`;
      }
      default:
        return party(who);
    }
  });
  let waits = $derived(item.form === "conversation" && (item.your_turn || (item.new ?? 0) > 0));
</script>

<li class="line">
  <span class="mark" aria-hidden="true">{initials(who)}</span>
  <div class="body">
    <a class="title" href={href.item(item.id)}>{item.title}</a>
    <p class="detail">
      {detail}{#if date}<span class="due {date.tone}">, {date.text}</span>{/if}
    </p>
    {#if note}<p class="note">{note}</p>{/if}
    {#if twins.length}
      <p class="twins">
        Maybe the same as {#each twins as id, i (id)}{i ? ", " : ""}<a href={href.item(id)}>{id}</a>{/each}
      </p>
    {/if}
  </div>
  {#if waits}<span class="dot" title={item.your_turn ? "Your turn" : `${item.new} new`}><span class="hidden">{item.your_turn ? "Your turn" : `${item.new} new`}</span></span>{/if}
  <a class="source" href={href.mail(origin(item))} aria-label="Open the source mail" title="Open the source mail">
    <svg viewBox="0 0 20 20" width="20" height="20" aria-hidden="true">
      <rect x="2.5" y="4.5" width="15" height="11" rx="2" fill="none" stroke="currentColor" stroke-width="1.5" />
      <path d="M3 5.5l7 5.5 7-5.5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round" />
    </svg>
  </a>
</li>

<style>
  .line {
    position: relative;
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-2);
    margin: 0 calc(-1 * var(--space-2));
    border-radius: var(--radius-md);
    transition: background var(--transition-fast);
  }
  .line:hover {
    background: var(--color-background-surface);
  }
  .mark {
    flex: none;
    display: grid;
    place-items: center;
    width: var(--space-8);
    height: var(--space-8);
    border-radius: var(--radius-full);
    background: var(--color-background-surface);
    border: 1px solid var(--color-border-subtle);
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
    font-weight: 600;
  }
  .body {
    flex: 1;
    min-width: 0;
  }
  .title {
    color: var(--color-text);
    text-decoration: none;
    font-weight: 500;
  }
  /* The title link covers the line, so the whole line opens the item. */
  .title::after {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: var(--radius-md);
  }
  .title:focus-visible {
    outline: none;
  }
  .title:focus-visible::after {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }
  .detail {
    margin: 0;
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    overflow: hidden;
  }
  .note,
  .twins {
    margin: var(--space-1) 0 0;
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  .twins a {
    position: relative;
    z-index: 1;
    color: var(--color-text-muted);
  }
  .due::after {
    content: "";
    display: inline-block;
    width: var(--space-2);
    height: var(--space-2);
    margin: 0 0 1px var(--space-2);
    border-radius: var(--radius-full);
    background: transparent;
  }
  .due.soon::after {
    background: var(--color-warning);
  }
  .due.late::after {
    background: var(--color-error);
  }
  .dot {
    flex: none;
    width: var(--space-2);
    height: var(--space-2);
    border-radius: var(--radius-full);
    background: var(--color-unread);
  }
  .hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }
  .source {
    position: relative;
    z-index: 1;
    flex: none;
    display: grid;
    place-items: center;
    width: var(--space-8);
    height: var(--space-8);
    border-radius: var(--radius-md);
    color: var(--color-text-faint);
    transition: color var(--transition-fast), background var(--transition-fast);
  }
  .source:hover,
  .source:focus-visible {
    color: var(--color-text);
    background: var(--color-background-elevated);
    outline: none;
  }
  .source:focus-visible {
    box-shadow: 0 0 0 2px var(--color-primary);
  }
  @media (prefers-reduced-motion: reduce) {
    .line,
    .source {
      transition: none;
    }
  }
</style>
