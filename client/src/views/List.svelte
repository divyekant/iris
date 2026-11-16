<!-- The full list of one form, newest first, 50 items for each page. -->
<script lang="ts">
  import { get, type Page } from "../api";
  import Line from "./Line.svelte";

  let { form, page }: { form: string; page: Page } = $props();
  const TITLES: Record<string, string> = {
    commitment: "Commitments",
    conversation: "Conversations",
    feed: "Feed",
    signal: "Signals",
  };

  // svelte-ignore state_referenced_locally
  let items = $state(page.items);
  // svelte-ignore state_referenced_locally
  let next = $state(page.next);
  let error = $state("");

  async function more() {
    try {
      const nextPage = await get<Page>(`items?form=${form}&limit=50&before=${encodeURIComponent(next ?? "")}`);
      items = [...items, ...nextPage.items];
      next = nextPage.next;
    } catch (e) {
      error = (e as Error).message;
    }
  }
</script>

<h1>{TITLES[form] ?? form}</h1>
{#if items.length === 0}
  <p class="empty">Nothing here yet.</p>
{:else}
  <ul>
    {#each items as item (item.id)}<Line {item} />{/each}
  </ul>
{/if}
{#if next}<button type="button" onclick={more}>More</button>{/if}
{#if error}<p role="alert">{error}</p>{/if}

<style>
  h1 {
    font-size: var(--font-size-3);
    margin: 0 0 var(--space-6);
  }
  ul {
    list-style: none;
    margin: 0 0 var(--space-4);
    padding: 0;
  }
  .empty {
    color: var(--color-text-muted);
  }
  button {
    font: inherit;
    color: var(--color-text);
    background: var(--color-background-surface);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    padding: var(--space-2) var(--space-4);
    cursor: pointer;
  }
</style>
