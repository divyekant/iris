<!--
  Spec 1 section 4.7: Google refused the refresh token of an account, so its
  sync stopped. The brief and the setup flow name each such account.
-->
<script lang="ts">
  import type { Account } from "../api";

  let { accounts }: { accounts: Account[] } = $props();
  let locked = $derived(accounts.filter((a) => a.state === "needs_sign_in"));
</script>

{#each locked as account (account.id)}
  <div class="locked" role="alert">
    <p>
      Google stopped the sign-in of {account.address}. Iris copies no new mail from this account until you sign in again.
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
