<!--
  The account of an item (card R6-02, option B, the owner's pick): a quiet pill
  with the short name of the account. It shows only with two or more accounts.
  Each view that shows an item in the brief uses it: a line and a login code.
-->
<script lang="ts">
  import type { Item } from "../api";
  import { access } from "../access.svelte.ts";

  let { item }: { item: Item } = $props();
  let name = $derived(item.account ? access.accounts[item.account] : undefined);
</script>

{#if name}<span class="pill" title={`From your account ${name}`}>{name}</span>{/if}

<style>
  .pill {
    flex: none;
    padding: 0 var(--space-2);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-full);
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
</style>
