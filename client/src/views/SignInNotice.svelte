<!--
  Spec 1 section 4.7: Google refused the refresh token of an account, so its
  sync stopped. App.svelte shows this above every view, from the access state.
-->
<script lang="ts">
  import { access } from "../access.svelte.ts";
</script>

{#each access.locked as address (address)}
  <div class="locked" role="alert">
    <p>
      Google stopped the sign-in of {address}. Iris copies no new mail from this account until you sign in again.
      On the machine that runs Iris, run <code>iris account add client.json</code> with the same client file.
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
