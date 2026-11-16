<!--
  The jobs of the agents (card R4-09): the open jobs by state, the jobs that
  ask you first, then the jobs that ended. Each line opens the job.
-->
<script lang="ts">
  import { get, own, type Job, type JobPage, type JobState } from "../api";
  import { due, now } from "../jobs";
  import { href } from "../route";

  let { open: firstOpen, done: firstDone }: { open: JobPage; done: JobPage } = $props();

  // svelte-ignore state_referenced_locally
  let open = $state(firstOpen.jobs);
  // svelte-ignore state_referenced_locally
  let nextOpen = $state(firstOpen.next);
  // svelte-ignore state_referenced_locally
  let done = $state(firstDone.jobs);
  // svelte-ignore state_referenced_locally
  let nextDone = $state(firstDone.next);
  let busy = $state("");
  let error = $state("");

  const GROUPS: [JobState, string][] = [
    ["needs_you", "Asks you"],
    ["working", "Working"],
    ["waiting", "Waiting"],
  ];

  async function more(which: "open" | "done") {
    busy = which;
    error = "";
    try {
      const after = encodeURIComponent((which === "open" ? nextOpen : nextDone) ?? "");
      const page = await get<JobPage>(`jobs?state=${which}&limit=50&after=${after}`);
      if (which === "open") [open, nextOpen] = [[...open, ...page.jobs], page.next];
      else [done, nextDone] = [[...done, ...page.jobs], page.next];
    } catch (e) {
      error = own(e);
    } finally {
      busy = "";
    }
  }
</script>

{#snippet line(job: Job)}
  {@const limit = due(job)}
  <li>
    <a class="job" href={href.job(job.id)}>
      <span class="mark {job.state}" aria-hidden="true"></span>
      <span class="what">
        <span class="goal">{job.goal}</span>
        <span class="detail">{job.state === "needs_you" ? job.question : now(job)}{#if limit}{" "}<span class={limit.tone}>{limit.text}.</span>{/if}</span>
      </span>
    </a>
  </li>
{/snippet}

<h1>Jobs</h1>
<p class="lead">The goals that you gave to your agents. Each job keeps each step, so you can see what the agent did and why.</p>

{#if open.length === 0 && done.length === 0}
  <p class="empty">No job yet. A job is a goal with limits, for example “Get the signed contract from Dana before 20 Nov.”</p>
{/if}

{#each GROUPS as [state, title] (state)}
  {@const jobs = open.filter((j) => j.state === state)}
  {#if jobs.length}
    <section aria-labelledby={state}>
      <h2 id={state}><span>{title}</span> <span class="count">{jobs.length}</span></h2>
      <ul>{#each jobs as job (job.id)}{@render line(job)}{/each}</ul>
    </section>
  {/if}
{/each}
{#if nextOpen}<button type="button" onclick={() => more("open")} disabled={!!busy}>{busy === "open" ? "Loading" : "Show more open jobs"}</button>{/if}

{#if done.length}
  <section aria-labelledby="done">
    <h2 id="done"><span>Ended</span></h2>
    <ul>{#each done as job (job.id)}{@render line(job)}{/each}</ul>
    {#if nextDone}<button type="button" onclick={() => more("done")} disabled={!!busy}>{busy === "done" ? "Loading" : "Show more"}</button>{/if}
  </section>
{/if}
{#if error}<p role="alert">{error}</p>{/if}

<style>
  h1 {
    font-size: var(--font-size-3);
    font-weight: 500;
    letter-spacing: -0.01em;
    margin: var(--space-4) 0 var(--space-2);
  }
  .lead,
  .empty {
    color: var(--color-text-muted);
    margin: 0 0 var(--space-8);
    max-width: 60ch;
  }
  section {
    margin-bottom: var(--space-12);
  }
  h2 {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    font-size: var(--font-size-0);
    font-weight: 600;
    margin: 0 0 var(--space-2);
    padding-bottom: var(--space-2);
    border-bottom: 1px solid var(--color-border);
  }
  .count {
    color: var(--color-text-muted);
    font-weight: 400;
    font-variant-numeric: tabular-nums;
  }
  ul {
    list-style: none;
    margin: 0 0 var(--space-4);
    padding: 0;
  }
  .job {
    display: flex;
    gap: var(--space-3);
    padding: var(--space-3) 0;
    border-bottom: 1px solid var(--color-border-subtle);
    text-decoration: none;
  }
  .job:hover .goal {
    text-decoration: underline;
  }
  .mark {
    flex: none;
    width: 8px;
    height: 8px;
    margin-top: 8px;
    border-radius: var(--radius-full);
    border: 2px solid var(--color-text-faint);
    box-sizing: border-box;
  }
  .mark.needs_you {
    border-color: var(--color-primary);
    background: var(--color-primary);
  }
  .mark.working {
    border-color: var(--color-primary);
  }
  .mark.done {
    border-color: var(--color-text-faint);
    background: var(--color-text-faint);
  }
  .what {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .detail {
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .soon,
  .late {
    color: var(--color-text);
  }
  button {
    font: inherit;
    color: var(--color-text);
    background: var(--color-background-elevated);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    padding: var(--space-2) var(--space-4);
    cursor: pointer;
  }
  button:hover {
    border-color: var(--color-primary);
  }
</style>
