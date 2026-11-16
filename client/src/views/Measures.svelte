<!-- The measures page. Release 1 has one measure: the open count of source mail (R1-22). -->
<script lang="ts">
  import { readOpens, type Opens } from "../opens";
  import { when } from "../format";

  let { opens: initial = null }: { opens?: Opens | null } = $props();
  // svelte-ignore state_referenced_locally
  let opens = $state(initial ?? { count: 0, since: null });
  $effect(() => {
    if (!initial) opens = readOpens();
  });
</script>

<h1>Measures</h1>
<section aria-labelledby="opens">
  <h2 id="opens">Source mail opened</h2>
  <p class="number">{opens.count}</p>
  <p class="note">
    {opens.since ? `Since ${when(opens.since)}.` : "No opens yet."} A low number is good: the items tell you enough without the mail.
    This count stays in this browser. Nothing leaves the machine.
  </p>
</section>

<style>
  h1 {
    font-size: var(--font-size-3);
    margin: 0 0 var(--space-6);
  }
  h2 {
    font-size: var(--font-size-1);
    margin: 0;
  }
  .number {
    font-size: var(--font-size-4);
    font-weight: 600;
    margin: var(--space-2) 0;
  }
  .note {
    color: var(--color-text-muted);
    max-width: 60ch;
  }
</style>
