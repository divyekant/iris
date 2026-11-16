<!--
  One bundle on one line (spec 2 section 6.4): the items of one `bundle` rule.
  The mark is a stack with the count. The detail names who sent them.
  The whole line opens the list of the bundle.
-->
<script lang="ts">
  import type { Bundle } from "../api";
  import { other, party, partyKey } from "../format";
  import { href } from "../route";

  let { bundle }: { bundle: Bundle } = $props();

  const NAMES = 3;
  let detail = $derived.by(() => {
    const names = new Map<string, string>();
    for (const item of bundle.items) {
      const who = other(item);
      names.set(partyKey(who), party(who));
    }
    const list = [...names.values()];
    const shown = list.slice(0, NAMES).join(", ");
    // The API gives the newest items only: then the count of senders is not known.
    if (bundle.items.length < bundle.count) return `From ${shown} and others`;
    const rest = list.length - NAMES;
    return rest > 0 ? `From ${shown} and ${rest} ${rest === 1 ? "other" : "others"}` : `From ${shown}`;
  });
</script>

<li class="line">
  <span class="mark" aria-hidden="true">{bundle.count}</span>
  <div class="body">
    <a class="title" href={href.bundle(bundle.rule)}>{bundle.name}<span class="hidden">, {bundle.count} items</span></a>
    <p class="detail">{detail}</p>
  </div>
  <span class="go" aria-hidden="true">
    <svg viewBox="0 0 20 20" width="20" height="20">
      <path d="M8 5l5 5-5 5" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" />
    </svg>
  </span>
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
  /* A stack: a second round mark shows behind the first. */
  .mark {
    flex: none;
    display: grid;
    place-items: center;
    width: var(--space-8);
    height: var(--space-8);
    margin-right: var(--space-1);
    border-radius: var(--radius-full);
    background: var(--color-background-surface);
    border: 1px solid var(--color-border-subtle);
    box-shadow:
      var(--space-1) 0 0 -1px var(--color-background-elevated),
      var(--space-1) 0 0 0 var(--color-border);
    color: var(--color-text);
    font-size: var(--font-size--1);
    font-weight: 600;
    font-variant-numeric: tabular-nums;
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
  /* The title link covers the line, so the whole line opens the bundle. */
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
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }
  .go {
    flex: none;
    display: grid;
    place-items: center;
    width: var(--space-8);
    height: var(--space-8);
    color: var(--color-text-faint);
    transition: color var(--transition-fast);
  }
  .line:hover .go {
    color: var(--color-text);
  }
  @media (prefers-reduced-motion: reduce) {
    .line,
    .go {
      transition: none;
    }
  }
</style>
