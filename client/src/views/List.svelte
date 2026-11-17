<!--
  A full list, 50 items for each page: all items of one form, or one section of the brief.
  With `act` (a section or a bundle of the brief), each line has the actions of a brief line:
  the quick Done and the quiet verb. A line that the person acts on leaves the list.
-->
<script lang="ts">
  import { flushHolds } from "../act";
  import { get, own, type Page } from "../api";
  import { watch } from "../clock";
  import { addPage, nextExpiry, unexpired } from "../format";
  import Line from "./Line.svelte";

  // `more` is the API path of the list. `cursor` names the parameter that takes `next`:
  // `before` for the lists of a form, `after` for the sections of the brief. Send `next` back unchanged.
  // `expiring`: the Signals section of the brief, where a signal leaves at its expiry (spec 1 section 8).
  let {
    title,
    page,
    more = "",
    cursor = "before",
    expiring = false,
    act = false,
    delay,
    now: start = new Date(),
  }: {
    title: string;
    page: Page;
    more?: string;
    cursor?: "before" | "after";
    expiring?: boolean;
    act?: boolean;
    delay?: number;
    now?: Date;
  } = $props();
  // svelte-ignore state_referenced_locally
  let now = $state(start);

  // svelte-ignore state_referenced_locally
  let items = $state(page.items);
  // svelte-ignore state_referenced_locally
  let next = $state(page.next);
  // svelte-ignore state_referenced_locally
  let duplicates = $state(page.duplicates ?? {});
  let error = $state("");
  let busy = $state(false);
  let shown = $derived(expiring ? unexpired(items, now) : items);
  // Keep the time left of each signal line fresh until the last expiry.
  let until = $derived(nextExpiry(items, now));
  $effect(() => (until ? watch(until, (d) => (now = d)) : undefined));

  const gone = (id: string) => (items = items.filter((i) => i.id !== id));
  // A held action outlives its line. If the person leaves the list or the page, the click stands: send it now.
  $effect(() => {
    if (!act) return;
    window.addEventListener("pagehide", flushHolds);
    return () => {
      window.removeEventListener("pagehide", flushHolds);
      flushHolds();
    };
  });

  async function loadMore() {
    busy = true;
    error = "";
    try {
      const nextPage = await get<Page>(`${more}&${cursor}=${encodeURIComponent(String(next ?? ""))}`);
      items = addPage(items, nextPage.items);
      duplicates = { ...duplicates, ...nextPage.duplicates };
      next = nextPage.next;
    } catch (e) {
      error = own(e);
    } finally {
      busy = false;
    }
  }
</script>

<h1>{title}</h1>
{#if shown.length === 0}
  <p class="empty">Nothing here now. New items show here after the next sync.</p>
{:else}
  <ul>
    {#each shown as item (item.id)}<Line {item} {now} twins={duplicates[item.id]} ondone={act ? gone : undefined} {delay} />{/each}
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
