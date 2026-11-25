<!--
  The jobs of the agents (card R4-09): the jobs that ask you first, then the
  jobs that work, wait and ended. Each state has its own page from the API,
  so an older job that asks you is never behind a page of newer jobs.
  Each line opens the job.
-->
<script lang="ts">
  import { get, own, type Job, type JobPage, type JobState } from "../api";
  import { due, idle, now, pageSize, startJob } from "../jobs";
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
  // The ID and the draft stay in the storage of this tab until the start ends, so a retry after a
  // lost answer and a reload sends the same ID.
  const DRAFT = "iris.newJob";
  type Draft = { request: string; goal: string; day: string };
  function storage(): Storage | undefined {
    try {
      return globalThis.sessionStorage;
    } catch {
      return undefined;
    }
  }
  function restore(): Draft | null {
    try {
      const v = JSON.parse(storage()?.getItem(DRAFT) ?? "null");
      return v && typeof v.request === "string" && v.request && typeof v.goal === "string" && typeof v.day === "string" ? v : null;
    } catch {
      return null;
    }
  }
  function keep(d: Draft | null) {
    try {
      if (d) storage()?.setItem(DRAFT, JSON.stringify(d));
      else storage()?.removeItem(DRAFT);
    } catch {
      // A blocked storage keeps the ID only in this page.
    }
  }
  const kept = restore();
  let request = $state<string | null>(kept?.request ?? null);
  function begin() {
    request = globalThis.crypto.randomUUID();
    keep({ request, goal: "", day: "" });
  }
  function end() {
    request = null;
    keep(null);
  }
  async function start(goal: string, dueAt: string | null): Promise<string> {
    try {
      const job = await startJob(goal, dueAt, request!);
      end();
      location.hash = href.job(job.id);
      return "";
    } catch (e) {
      const err = e as Error & { status?: number; body?: { job?: string } };
      if (err.status === 409 && err.body?.job) {
        end();
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
      pages[state] = {
        jobs: [...pages[state].jobs, ...page.jobs.filter((j) => !seen.has(j.id))],
        next: page.next,
        // The sessions of the jobs of the new page join those of the pages before.
        // A page with no map (an older engine) keeps the map of the other pages.
        sessions: { ...pages[state].sessions, ...page.sessions },
      };
    } catch (e) {
      error = own(e);
    } finally {
      busy = "";
    }
  }
</script>

{#snippet line(job: Job, page: JobPage)}
  {@const limit = due(job)}
  <!-- A working job that no agent works on says so (issue 791), as its page does. -->
  {@const off = job.state === "working" ? idle(page, job) : null}
  <li>
    <a class="job" href={href.job(job.id)}>
      <span class="mark {job.state}" class:idle={off} aria-hidden="true"></span>
      <span class="what">
        <span class="goal">{job.goal}</span>
        <span class="detail">{job.state === "needs_you" ? job.question : off ? [off.said, off.ended].filter(Boolean).join(" ") : now(job)}{#if limit}{" "}<span class={limit.tone}>{limit.text}.</span>{/if}</span>
      </span>
    </a>
  </li>
{/snippet}

<div class="top">
  <h1>Jobs</h1>
  {#if !request}<button type="button" onclick={begin}>New job</button>{/if}
</div>
<p class="lead">The goals that you gave to your agents. Each job keeps each step, so you can see what the agent did and why.</p>

{#if request}
  <JobForm
    action="Start the job"
    goal={kept?.request === request ? kept.goal : ""}
    day={kept?.request === request ? kept.day : ""}
    save={start}
    oncancel={end}
    ondraft={(goal, day) => request && keep({ request, goal, day })}
  />
{/if}

{#if empty && !request}
  <p class="empty">No job yet. A job is a goal with limits, for example “Get the signed contract from Dana before 20 Nov.”</p>
{/if}

{#each GROUPS as [state, title] (state)}
  {@const page = pages[state]}
  {#if page.jobs.length}
    <section aria-labelledby={state}>
      <h2 id={state}><span>{title}</span></h2>
      <ul>{#each page.jobs as job (job.id)}{@render line(job, page)}{/each}</ul>
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
  /* No agent works on the job: the mark does not say "works" (issue 791). */
  .mark.working.idle {
    border-color: var(--color-text-faint);
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
