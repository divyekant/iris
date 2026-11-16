<!-- Records: one table for each type. The engine gives 50 records of one type at a time. -->
<script lang="ts">
  import { get, type Page } from "../api";
  import RecordTable from "./RecordTable.svelte";

  let { counts, type = "", page = null }: { counts: Record<string, number>; type?: string; page?: Page | null } = $props();
  let types = $derived(Object.entries(counts).filter(([, n]) => n > 0));
  // svelte-ignore state_referenced_locally
  let selected = $state(type || Object.entries(counts).find(([, n]) => n > 0)?.[0] || "");
</script>

<h1>Records</h1>
{#if types.length === 0}
  <p class="empty">No records yet.</p>
{:else}
  <div class="types" role="group" aria-label="Record types">
    {#each types as [name, n] (name)}
      <button type="button" aria-pressed={selected === name} onclick={() => (selected = name)}>{name} <span>{n}</span></button>
    {/each}
  </div>
  {#key selected}
    {#if page && selected === type}
      <RecordTable type={selected} items={page.items} />
    {:else}
      {#await get<Page>(`items?form=record&type=${encodeURIComponent(selected)}&limit=50`) then result}
        <RecordTable type={selected} items={result.items} />
      {:catch error}
        <p role="alert">{error.message}</p>
      {/await}
    {/if}
  {/key}
{/if}

<style>
  h1 {
    font-size: var(--font-size-3);
    margin: 0 0 var(--space-6);
  }
  .empty {
    color: var(--color-text-muted);
  }
  .types {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2);
    margin-bottom: var(--space-4);
  }
  button {
    font: inherit;
    color: var(--color-text);
    background: var(--color-background-surface);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-full);
    padding: var(--space-1) var(--space-3);
    cursor: pointer;
    text-transform: capitalize;
  }
  button[aria-pressed="true"] {
    border-color: var(--color-primary);
  }
  button span {
    color: var(--color-text-muted);
  }
</style>
