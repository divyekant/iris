<!--
  The jobs of the agents (card R4-09): the jobs that ask you first, then the
  jobs that work, wait and ended. Each state has its own page from the API,
  so an older job that asks you is never behind a page of newer jobs.
  Each line opens the job.
-->
<script lang="ts">
  import { get, own, type Job, type JobPage, type JobState } from "../api";
  import { due, now, pageSize, startJob } from "../jobs";
  import { href } from "../route";
  import JobForm from "./JobForm.svelte";

  let { pages: first }: { pages: Record<JobState, JobPage> } = $props();

  const GROUPS: [JobState, string][] = [
    ["needs_you", "Asks you"],
    ["working", "Working"],
    ["waiting", "Waiting"],
    ["done", "Ended"],
  ];

  // svelte-ignore state_referenced_locally
  let pages = $state(structuredClone(first));
  let busy = $state("");
  let error = $state("");
  let empty = $derived(GROUPS.every(([state]) => pages[state].jobs.length === 0));

  // One request ID for each open form: a retry of the same start gives the same job (decision D25).
  let request = $state<string | null>(null);
  async function start(goal: string, dueAt: string | null): Promise<string> {
    try {
      const job = await startJob(goal, dueAt, request!);
      location.hash = href.job(job.id);
      return "";
    } catch (e) {
      const err = e as Error & { status?: number; body?: { job?: string } };
      if (err.status === 409 && err.body?.job) {
        location.hash = href.job(err.body.job);
        return "";
      }
      return own(e) || err.message;
    }
  }

  async function more(state: JobState) {
    busy = state;
    error = "";
    try {
      const page = await get<JobPage>(`jobs?state=${state}&limit=${pageSize(state)}&after=${encodeURIComponent(pages[state].next ?? "")}`);
      const seen = new Set(pages[state].jobs.map((j) => j.id));
      pages[state] = { jobs: [...pages[state].jobs, ...page.jobs.filter((j) => !seen.has(j.id))], next: page.next };
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

<div class="top">
  <h1>Jobs</h1>
  {#if !request}<button type="button" onclick={() => (request = globalThis.crypto.randomUUID())}>New job</button>{/if}
</div>
<p class="lead">The goals that you gave to your agents. Each job keeps each step, so you can see what the agent did and why.</p>

{#if request}
  <JobForm action="Start the job" save={start} oncancel={() => (request = null)} />
{/if}

{#if empty && !request}
  <p class="empty">No job yet. A job is a goal with limits, for example “Get the signed contract from Dana before 20 Nov.”</p>
{/if}

{#each GROUPS as [state, title] (state)}
  {@const page = pages[state]}
  {#if page.jobs.length}
    <section aria-labelledby={state}>
      <h2 id={state}><span>{title}</span></h2>
      <ul>{#each page.jobs as job (job.id)}{@render line(job)}{/each}</ul>
      {#if page.next}<button type="button" onclick={() => more(state)} disabled={!!busy}>{busy === state ? "Loading" : "Show more"}</button>{/if}
    </section>
  {/if}
{/each}
{#if error}<p role="alert">{error}</p>{/if}

<style>
  .top {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--space-4);
  }
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
