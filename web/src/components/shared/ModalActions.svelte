<script lang="ts">
  import type { Snippet } from 'svelte';

  let {
    children,
  }: {
    children: Snippet;
  } = $props();
</script>

<div class="modal-actions">
  {@render children()}
</div>

<style>
  .modal-actions {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 16px 24px;
    border-top: 1px solid var(--iris-color-border);
    margin: 0 -24px -24px -24px;
  }
</style>
