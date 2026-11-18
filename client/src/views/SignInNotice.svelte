<!--
  Spec 1 section 4.7: the provider refused the sign-in of an account, so its
  sync stopped. App.svelte shows this above every view, from the access state.
-->
<script lang="ts">
  import { access } from "../access.svelte.ts";
  import { signIn } from "../format";
</script>

{#each access.locked as a (a.address)}
  {@const s = signIn(a)}
  <div class="locked" role="alert">
    <p>
      {s.who} stopped the sign-in of {a.address}. Iris copies no new mail from this account until you sign in again.
      On the machine that runs Iris, run <code>{s.command}</code> {s.detail}.
    </p>
  </div>
{/each}

<style>
  .locked {
    margin-bottom: var(--space-8);
    border-left: 4px solid var(--color-warning);
    padding-left: var(--space-3);
  }
  .locked p {
    margin: 0;
  }
</style>
