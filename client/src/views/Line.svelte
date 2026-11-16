<!-- One line of the brief or of a list: the title opens the item, "Mail" opens its source. -->
<script lang="ts">
  import type { Item } from "../api";
  import { money, origin, when, who } from "../format";
  import { href } from "../route";

  let { item }: { item: Item } = $props();
  let detail = $derived.by(() => {
    const parts = [who(item)];
    if (item.due) parts.push(`due ${when(item.due.date ?? item.due.at)}`);
    if (item.amount) parts.push(money(item.amount));
    if (item.form === "conversation" && item.your_turn) parts.push("your turn");
    if (item.form === "conversation" && item.new) parts.push(`${item.new} new`);
    if (item.form === "feed") parts.push(when(item.published));
    return parts.filter(Boolean).join(" · ");
  });
</script>

<li>
  <a class="title" href={href.item(item.id)}>{item.title}</a>
  <span class="detail">{detail}</span>
  <a class="source" href={href.mail(origin(item))}>Mail</a>
</li>

<style>
  li {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--space-1) var(--space-3);
    padding: var(--space-2) 0;
    border-bottom: 1px solid var(--color-border-subtle);
  }
  .title {
    text-decoration: none;
    font-weight: 500;
  }
  .title:hover {
    text-decoration: underline;
  }
  .detail {
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
    flex: 1;
  }
  .source {
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
</style>
