<!--
  The grants inside one job (card R4-10): the changes that the agent of this
  job can make with no review. Each grant is one rule of policy.toml with
  this job. Deny and review rules, and the fixed safety limits, still apply.
-->
<script lang="ts">
  import { post, type Job, type Policy, type Rule } from "../api";
  import { verbs as said } from "../rules";
  import { GRANTS, grantsOf, ruleOf } from "../start";

  let { job, policy: given }: { job: Job; policy: Policy } = $props();
  // svelte-ignore state_referenced_locally
  let policy = $state(given);
  let busy = $state(false);
  let problem = $state("");

  let open = $derived(job.state !== "done");
  let mine = $derived(grantsOf(policy.rules, job.id));
  // A grant with other verbs (for example from an edit of the file) shows as its own line.
  let others = $derived(mine.filter((r) => !GRANTS.some((g) => ruleOf([r], g))));

  async function change(path: string, body: object) {
    busy = true;
    problem = "";
    try {
      policy = await post<Policy>(path, { version: policy.version, ...body });
    } catch (error) {
      problem = (error as Error).message;
    } finally {
      busy = false;
    }
  }
  const allow = (verbs: readonly string[]) => change("policy/grant", { job: job.id, verbs });
  const remove = (rule: Rule) => change("policy/remove", { id: rule.id });

  function toggle(group: (typeof GRANTS)[number]) {
    const rule = ruleOf(mine, group);
    if (rule) remove(rule);
    else allow(group.verbs);
  }
</script>

<section aria-labelledby="grants">
  <h2 id="grants">Without asking you</h2>
  <p class="note">
    {#if open}The agent asks you before each change to your mail. Allow some changes in this job only.{:else}The job ended, so these grants allow nothing more.{/if}
    {#if policy.mode === "observe"}Iris is in observe mode now: it applies nothing, also with a grant.{/if}
  </p>

  <ul class="groups">
    {#each GRANTS as g (g.key)}
      {@const on = !!ruleOf(mine, g)}
      <li>
        <label class:on>
          <span class="text">
            <span class="title">{g.title}</span>
            <span class="what">{g.note}</span>
          </span>
          <input type="checkbox" role="switch" checked={on} disabled={busy || (!open && !on)} onchange={() => toggle(g)} />
        </label>
      </li>
    {/each}
    {#each others as r (r.id)}
      <li class="other">
        <span class="text"><span class="title">{said(r)}</span><span class="what">From your policy file, rule {r.id}</span></span>
        <button class="quiet" type="button" disabled={busy} onclick={() => remove(r)}>Remove</button>
      </li>
    {/each}
  </ul>
  {#if problem}<p class="error" role="alert">{problem}</p>{/if}
</section>

<style>
  section {
    margin: 0 0 var(--space-12);
  }
  h2 {
    font-size: var(--font-size-0);
    font-weight: 600;
    margin: 0 0 var(--space-2);
    padding-bottom: var(--space-2);
    border-bottom: 1px solid var(--color-border);
  }
  p {
    margin: 0;
  }
  .note {
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
    margin-bottom: var(--space-3);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .groups li label,
  .other {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    padding: var(--space-3) 0;
    border-bottom: 1px solid var(--color-border-subtle);
  }
  .groups label {
    cursor: pointer;
  }
  .text {
    display: grid;
    gap: var(--space-1);
  }
  .what {
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  /* A switch from a native checkbox. */
  input[role="switch"] {
    appearance: none;
    flex: none;
    width: calc(var(--space-8) + var(--space-2));
    height: var(--space-6);
    margin: 0;
    border-radius: var(--radius-full);
    background: var(--color-border);
    position: relative;
    cursor: pointer;
    transition: background var(--transition-normal);
  }
  input[role="switch"]::after {
    content: "";
    position: absolute;
    top: var(--space-1);
    left: var(--space-1);
    width: var(--space-4);
    height: var(--space-4);
    border-radius: var(--radius-full);
    background: var(--color-background);
    transition: transform var(--transition-normal);
  }
  input[role="switch"]:checked {
    background: var(--color-primary);
  }
  input[role="switch"]:checked::after {
    transform: translateX(var(--space-4));
  }
  input[role="switch"]:focus-visible,
  button:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }
  input:disabled {
    opacity: 0.5;
    cursor: default;
  }
  button {
    font: inherit;
    cursor: pointer;
    min-height: calc(var(--space-8) + var(--space-2));
    border-radius: var(--radius-full);
  }
  .quiet {
    padding: 0 var(--space-3);
    border: 0;
    background: transparent;
    color: var(--color-text-muted);
  }
  .quiet:hover {
    color: var(--color-text);
  }
  .error {
    margin-top: var(--space-3);
    border-left: 4px solid var(--color-error);
    padding-left: var(--space-3);
  }
</style>
