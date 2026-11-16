<!-- Records: one table for each type, 50 records at a time, with "Show more" for the next page. -->
<script lang="ts">
  import { get, type Page } from "../api";
  import RecordTable from "./RecordTable.svelte";

  let { counts, type = "", page = null }: { counts: Record<string, number>; type?: string; page?: Page | null } = $props();
  let types = $derived(Object.entries(counts).filter(([, n]) => n > 0));

  // svelte-ignore state_referenced_locally
  let selected = $state(type && counts[type] ? type : (Object.entries(counts).find(([, n]) => n > 0)?.[0] ?? ""));
  // svelte-ignore state_referenced_locally
  let items = $state(page?.items ?? []);
  // svelte-ignore state_referenced_locally
  let next = $state(page?.next ?? null);
  // svelte-ignore state_referenced_locally
  let loaded = $state(page ? selected : "");
  let error = $state("");
  let busy = $state(false);

  async function load(name: string, before: string | null) {
    busy = true;
    error = "";
    try {
      const result = await get<Page>(`items?form=record&type=${encodeURIComponent(name)}&limit=50${before ? `&before=${encodeURIComponent(before)}` : ""}`);
      if (name !== selected) return;
      items = before ? [...items, ...result.items] : result.items;
      next = result.next;
      loaded = name;
    } catch (e) {
      error = (e as Error).message;
    } finally {
      busy = false;
    }
  }

  $effect(() => {
    if (selected && selected !== loaded) load(selected, null);
  });

  function choose(name: string) {
    selected = name;
    history.replaceState(null, "", `#/records/${name}`);
  }
</script>

<h1>Records</h1>
{#if types.length === 0}
  <p class="empty">No records yet. Receipts, bookings and other records show here after Iris reads them.</p>
{:else}
  <div class="types" role="group" aria-label="Type of record">
    {#each types as [name, n] (name)}
      <button type="button" aria-pressed={selected === name} onclick={() => choose(name)}>{name} <span>{n}</span></button>
    {/each}
  </div>
  {#if loaded === selected}
    <RecordTable type={selected} {items} />
    {#if next}<button class="more" type="button" onclick={() => load(selected, next)} disabled={busy}>{busy ? "Loading" : "Show more"}</button>{/if}
  {/if}
  {#if error}<p role="alert">{error}</p>{/if}
{/if}

<style>
  h1 {
    font-size: var(--font-size-3);
    font-weight: 500;
    letter-spacing: -0.01em;
    margin: var(--space-4) 0 var(--space-6);
  }
  .empty {
    color: var(--color-text-muted);
  }
  .types {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    margin-bottom: var(--space-6);
  }
  button {
    font: inherit;
    color: var(--color-text);
    background: var(--color-background-elevated);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-full);
    padding: var(--space-1) var(--space-4);
    cursor: pointer;
    transition: border-color var(--transition-fast);
  }
  .types button {
    text-transform: capitalize;
  }
  button span {
    color: var(--color-text-muted);
    margin-left: var(--space-1);
  }
  button[aria-pressed="true"],
  button:hover {
    border-color: var(--color-primary);
  }
  button:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }
  .more {
    margin-top: var(--space-4);
    border-radius: var(--radius-md);
  }
  @media (prefers-reduced-motion: reduce) {
    button {
      transition: none;
    }
  }
</style>
