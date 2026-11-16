<!--
  A full list: all items of one form (newest first, 50 for each page), or one
  section of the brief (the engine gives up to 200 items of a section).
-->
<script lang="ts">
  import { get, type Page } from "../api";
  import Line from "./Line.svelte";

  let { title, page, form = "", total = 0 }: { title: string; page: Page; form?: string; total?: number } = $props();

  // svelte-ignore state_referenced_locally
  let items = $state(page.items);
  // svelte-ignore state_referenced_locally
  let next = $state(page.next);
  let error = $state("");
  let busy = $state(false);

  async function more() {
    busy = true;
    error = "";
    try {
      const nextPage = await get<Page>(`items?form=${form}&limit=50&before=${encodeURIComponent(next ?? "")}`);
      items = [...items, ...nextPage.items];
      next = nextPage.next;
    } catch (e) {
      error = (e as Error).message;
    } finally {
      busy = false;
    }
  }
</script>

<h1>{title}</h1>
{#if items.length === 0}
  <p class="empty">Nothing here now. New items show here after the next sync.</p>
{:else}
  <ul>
    {#each items as item (item.id)}<Line {item} />{/each}
  </ul>
{/if}
{#if total > items.length}
  <p class="empty">The brief shows the first {items.length} of {total}. The full lists are under Commitments and Conversations.</p>
{/if}
{#if next && form}<button type="button" onclick={more} disabled={busy}>{busy ? "Loading" : "Show more"}</button>{/if}
{#if error}<p role="alert">{error}</p>{/if}

<style>
  h1 {
    font-size: var(--font-size-3);
    font-weight: 500;
    letter-spacing: -0.01em;
    margin: var(--space-4) 0 var(--space-6);
  }
  ul {
    list-style: none;
    margin: 0 0 var(--space-6);
    padding: 0;
  }
  .empty {
    color: var(--color-text-muted);
  }
  button {
    font: inherit;
    color: var(--color-text);
    background: var(--color-background-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    padding: var(--space-2) var(--space-4);
    cursor: pointer;
  }
  button:hover {
    border-color: var(--color-primary);
  }
  button:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }
</style>
