<!--
  The review screen (R2-06, spec 2 section 5.5): each open changeset, so that
  the person approves or denies what an agent or Iris asks to change.
-->
<script lang="ts">
  import type { Changeset, Mode } from "../api";
  import { author, shown, waits } from "../review";
  import Request from "./Request.svelte";

  let { changesets, mode = "apply" }: { changesets: Changeset[]; mode?: Mode } = $props();

  // Each open changeset shows, also one whose actions all have a decision. The headline counts only the waiting ones.
  let asks = $derived(changesets.filter((c) => shown(c).some(waits)));
  let count = $derived(asks.reduce((sum, c) => sum + shown(c).filter(waits).length, 0));
  let headline = $derived(
    count === 0
      ? "Nothing waits for your review."
      : asks.length === 1
        ? `${author(asks[0])} asks for ${count === 1 ? "one change" : `${count} changes`}.`
        : `${asks.length} requests ask for ${count} changes.`,
  );
</script>

<div class="opening">
  <p class="day">Review</p>
  <h1>{headline}</h1>
  {#if changesets.length === 0}
    <p class="empty">When an agent or Iris asks to change your mailbox, the request shows here with each message that it changes.</p>
  {/if}
</div>

{#if count > 0 && mode === "observe"}
  <p class="note">Iris is in observe mode. A change that you approve waits until you choose apply mode in <a href="#/rules">Rules</a>.</p>
{/if}

{#each changesets as c (c.id)}<Request changeset={c} />{/each}

<style>
  .opening {
    margin: var(--space-8) 0 var(--space-12);
  }
  .day,
  .empty {
    color: var(--color-text-muted);
    margin: 0 0 var(--space-3);
  }
  .empty {
    margin-top: var(--space-4);
    max-width: 48ch;
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
  .note {
    margin: 0 0 var(--space-8);
    padding-left: var(--space-3);
    border-left: 4px solid var(--color-warning);
    color: var(--color-text-muted);
  }
</style>
