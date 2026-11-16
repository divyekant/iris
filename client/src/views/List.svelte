<!-- A full list, 50 items for each page: all items of one form, or one section of the brief. -->
<script lang="ts">
  import { get, type Page } from "../api";
  import { addPage } from "../format";
  import Line from "./Line.svelte";

  // `more` is the API path of the list. `cursor` names the parameter that takes `next`:
  // `before` for the lists of a form, `after` for the sections of the brief. Send `next` back unchanged.
  let { title, page, more = "", cursor = "before" }: { title: string; page: Page; more?: string; cursor?: "before" | "after" } = $props();

  // svelte-ignore state_referenced_locally
  let items = $state(page.items);
  // svelte-ignore state_referenced_locally
  let next = $state(page.next);
  // svelte-ignore state_referenced_locally
  let duplicates = $state(page.duplicates ?? {});
  let error = $state("");
  let busy = $state(false);

  async function loadMore() {
    busy = true;
    error = "";
    try {
      const nextPage = await get<Page>(`${more}&${cursor}=${encodeURIComponent(String(next ?? ""))}`);
      items = addPage(items, nextPage.items);
      duplicates = { ...duplicates, ...nextPage.duplicates };
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
    {#each items as item (item.id)}<Line {item} twins={duplicates[item.id]} />{/each}
  </ul>
{/if}
{#if next && more}<button type="button" onclick={loadMore} disabled={busy}>{busy ? "Loading" : "Show more"}</button>{/if}
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
