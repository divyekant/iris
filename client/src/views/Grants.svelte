<!--
  The grants inside one job (card R4-10): the changes that the agent of this
  job can make with no review. Each grant is one rule of policy.toml with
  this job. Deny and review rules, and the fixed safety limits, still apply.
-->
<script lang="ts">
  import { get, post, type Job, type Policy, type Rule } from "../api";
  import { conditions, verbs as said } from "../rules";
  import { GRANTS, grantView } from "../start";

  let { job }: { job: Job } = $props();
  // The page reads the policy itself, so that a failed read shows with a retry, not as no grants.
  let policy = $state<Policy | null>(null);
  let failed = $state("");
  let busy = $state(false);
  let problem = $state("");

  // Invariant: the switches show only a policy that the page read after its last change. A failed read hides them.
  async function load() {
    failed = "";
    try {
      policy = await get<Policy>("policy");
    } catch (error) {
      policy = null;
      failed = (error as Error).message;
    }
  }
  $effect(() => {
    load();
  });

  let open = $derived(job.state !== "done");
  let view = $derived(policy ? grantView(policy.rules, job.id) : null);

  async function change(path: string, body: object) {
    if (!policy) return;
    busy = true;
    problem = "";
    try {
      policy = await post<Policy>(path, { version: policy.version, ...body });
    } catch (error) {
      problem = (error as Error).message;
      // The policy can be newer than the page (a 409), or the change can be done with a lost answer: read it again.
      await load();
    } finally {
      busy = false;
    }
  }
  const allow = (verbs: readonly string[]) => change("policy/grant", { job: job.id, verbs });
  const remove = (rule: Rule) => change("policy/remove", { id: rule.id });

  function toggle(event: Event, group: (typeof GRANTS)[number]) {
    const rule = view?.switches[group.key];
    // The switch shows the policy, not the click: it moves when the policy changes.
    (event.currentTarget as HTMLInputElement).checked = !!rule;
    if (rule) remove(rule);
    else allow(group.verbs);
  }
</script>

<section aria-labelledby="grants">
  <h2 id="grants">Without asking you</h2>
  {#if failed}
    <p class="error" role="alert">{#if problem}{problem} {/if}Iris cannot read your policy now, so the grants of this job do not show. {failed}</p>
    <button class="quiet" type="button" onclick={() => ((problem = ""), load())}>Try again</button>
  {:else if policy && view}
    <p class="note">
      {#if !open}The job ended, so these grants allow nothing more.
      {:else}The agent asks you before each change to your mail, unless a grant of this job or a rule of your whole policy below allows it. Allow more changes in this job only.{/if}
      {#if policy.mode === "observe"}Iris is in observe mode now: it applies nothing, also with a grant.{/if}
    </p>

    <ul class="groups">
      {#each GRANTS as g (g.key)}
        {@const on = !!view.switches[g.key]}
        <li>
          <label class:on>
            <span class="text">
              <span class="title">{g.title}</span>
              <span class="what">{g.note}</span>
            </span>
            <input type="checkbox" role="switch" checked={on} disabled={busy || (!open && !on)} onchange={(e) => toggle(e, g)} />
          </label>
        </li>
      {/each}
      {#each view.others as r (r.id)}
        <li class="other">
          <span class="text"><span class="title">{said(r)}</span><span class="what">{[...conditions(r).filter((c) => c !== "By agent" && c !== `Job ${job.id}`), `From your policy file, rule ${r.id}`].join(". ")}</span></span>
          <button class="quiet" type="button" disabled={busy} onclick={() => remove(r)}>Remove</button>
        </li>
      {/each}
    </ul>
    {#if view.general.length}
      <h3>Also allowed by your whole policy</h3>
      <ul class="general">
        {#each view.general as r (r.id)}
          <li><span class="title">{said(r)}</span> <span class="what">{[...conditions(r), `rule ${r.id}`].join(". ")}</span></li>
        {/each}
      </ul>
      <p class="note"><a class="touch" href="#/rules">Change these rules</a></p>
    {/if}
    {#if problem}<p class="error" role="alert">{problem}</p>{/if}
  {/if}
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
  h3 {
    font-size: var(--font-size--1);
    font-weight: 600;
    margin: var(--space-4) 0 var(--space-2);
  }
  .general {
    margin-bottom: var(--space-2);
  }
  .general li {
    padding: var(--space-2) 0;
    border-bottom: 1px solid var(--color-border-subtle);
  }
  .error {
    margin-top: var(--space-3);
    border-left: 4px solid var(--color-error);
    padding-left: var(--space-3);
  }
</style>
