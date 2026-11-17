<!--
  "Handle this" (card R4-10, option B of the owner): the start of a job from
  one commitment or conversation, on its own page. The title and the quote of
  the item stay in view above the form.
-->
<script lang="ts">
  import type { Item } from "../api";
  import { href } from "../route";
  import Start from "./Start.svelte";

  let { item: given }: { item: Item } = $props();
  // svelte-ignore state_referenced_locally
  let item = $state(given);
  let quote = $derived(item.sources.find((s) => s.role === "origin" && s.quote)?.quote);
</script>

<p class="eyebrow">New job</p>
<h1>Give this to an agent</h1>
<figure>
  <p class="title"><a href={href.item(item.id)}>{item.title}</a></p>
  {#if quote}<blockquote>{quote}</blockquote>{/if}
</figure>
<Start {item} oncancel={() => (location.hash = href.item(item.id))} onitem={(next) => (item = next)} />

<style>
  .eyebrow {
    margin: var(--space-4) 0 var(--space-1);
    color: var(--color-text-faint);
    font-size: var(--font-size--1);
  }
  h1 {
    font-size: var(--font-size-3);
    font-weight: 500;
    letter-spacing: -0.01em;
    margin: 0 0 var(--space-6);
  }
  figure {
    margin: 0 0 var(--space-6);
    padding: var(--space-4) var(--space-6);
    border-left: 2px solid var(--color-primary);
  }
  .title {
    margin: 0;
    font-weight: 500;
  }
  .title a {
    text-decoration: none;
  }
  .title a:hover {
    text-decoration: underline;
  }
  blockquote {
    margin: var(--space-2) 0 0;
    color: var(--color-text-muted);
  }
</style>
