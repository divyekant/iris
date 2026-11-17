<!--
  Above the brief: the policy file changed and waits for a confirmation, or
  it has an error, or a confirmed file is gone (spec 2 section 6.3). Nothing shows while the file is the
  active policy, or when the API does not answer.
-->
<script lang="ts">
  import { get, type Policy } from "../api";
  import { follow } from "../clock";

  let { policy = null, every = 30_000 }: { policy?: Policy | null; every?: number } = $props();
  // svelte-ignore state_referenced_locally
  let current = $state(policy);
  // The person can edit the file while the brief is open: look again each `every` ms.
  $effect(() => {
    if (!policy) return follow(() => get<Policy>("policy"), (p) => (current = p), every);
  });
  let file = $derived(current?.file);
  // The source of the open suggestions: approvals (R5-01), choices (R5-02) or both.
  let from = $derived.by(() => {
    const kinds = new Set((current?.proposals ?? []).map((p) => (p.kind === "choices" ? "hide" : "approve")));
    return kinds.size > 1 ? "what you approve and hide" : kinds.has("hide") ? "what you hide" : "your approvals";
  });
</script>

{#if current?.error}
  <p class="notice bad">Iris cannot use its rules: line {current.error.line} of the policy has an error. <a href="#/rules">Open the rules</a></p>
{:else if file?.state === "invalid"}
  <p class="notice bad">
    Your policy file has an error{file.error.line ? ` on line ${file.error.line}` : ""}. Iris uses the rules from before. <a href="#/rules">Open the rules</a>
  </p>
{:else if file?.state === "missing" && current?.version !== null}
  <p class="notice">Your policy file is missing. Iris uses the rules from before. <a href="#/rules">Open the rules</a></p>
{:else if file?.state === "changed"}
  <p class="notice">Your policy file changed. Iris uses it after you confirm the change. <a href="#/rules">Review the change</a></p>
{:else if current?.proposals?.length}
  <p class="notice idea">
    Iris suggests {current.proposals.length === 1 ? "a rule" : `${current.proposals.length} rules`} from {from}. <a href="#/rules">See the suggestions</a>
  </p>
{/if}

<style>
  .notice {
    margin: 0 0 var(--space-8);
    border-left: 4px solid var(--color-warning);
    padding-left: var(--space-3);
  }
  .notice.bad {
    border-left-color: var(--color-error);
  }
  .notice.idea {
    border-left-color: var(--color-primary);
  }
</style>
