<!-- Layout B of R5-07 (temporary): the watches on a page of their own. -->
<script lang="ts">
  import { loadWatches, type Watch } from "../watches";
  import Arrived from "./Arrived.svelte";
  import Watches from "./Watches.svelte";

  let { watches: first }: { watches: Watch[] } = $props();
  // svelte-ignore state_referenced_locally
  let watches = $state(first);
  const reload = () => loadWatches().then((w) => (watches = w), () => {});
</script>

<h1>Watches</h1>
<p class="lead">Name a mail that you wait for. Iris tells you one time when it comes.</p>
<Watches watches={watches.filter((w) => w.state === "waits")} onchange={reload} open />
{#each watches.filter((w) => w.state === "arrived") as w (w.id)}<div class="gap"><Arrived watch={w} ondone={reload} /></div>{/each}

<style>
  h1 {
    font-size: var(--font-size-3);
    font-weight: 500;
    margin: var(--space-8) 0 var(--space-2);
  }
  .lead {
    color: var(--color-text-muted);
    margin: 0 0 var(--space-8);
  }
  .gap {
    margin-top: var(--space-4);
  }
</style>
