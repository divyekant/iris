<!--
  One job of an agent (card R4-09): the goal, its limits, the state now, and
  each step in order: the moves of the job and the changesets of the agent.
  The words come from jobs.ts. Text of the agent and of mail is data, and
  shows as text only.
-->
<script lang="ts">
  import { get, type JobDetail } from "../api";
  import { agent, clock, day, due, now, steps, waitMail } from "../jobs";
  import Grants from "./Grants.svelte";

  // `every`: the ms between two reads of a job whose session can still change (a prop for the tests).
  let { detail: first, every = 5000 }: { detail: JobDetail; every?: number } = $props();
  // svelte-ignore state_referenced_locally
  let detail = $state(first);
  // While a session starts, runs or stops, its state can change: read the job again (issue 742).
  $effect(() => {
    const s = detail.session;
    if (detail.job.state !== "working" || !s || s.state === "ended") return;
    const id = detail.job.id;
    let live = true;
    const timer = setTimeout(() => {
      get<JobDetail>(`jobs/${encodeURIComponent(id)}`).then(
        (d) => {
          if (live) detail = d;
        },
        () => {},
      );
    }, every);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  });

  let job = $derived(detail.job);
  let limit = $derived(due(job));
  let list = $derived(steps(detail));
  let mail = $derived(job.state === "waiting" ? waitMail(detail.threads, job.wait?.thread) : null);
  let idle = $derived(agent(detail));
  // The day shows above the first step of each day.
  const newDay = (i: number) => i === 0 || day(list[i - 1].at) !== day(list[i].at);
</script>

<p class="eyebrow">Job</p>
<h1>{job.goal}</h1>
<p class="limits">
  {#if limit}<span class={limit.tone}>{limit.text}{#if limit.tone}<span class="dot" aria-hidden="true"></span>{/if}</span>{/if}
  <span>Started {day(job.created)}</span>
</p>

<section class="now {job.state}" class:idle aria-label="Now">
  {#if job.state === "needs_you"}
    <p class="label">The agent asks you</p>
    <p class="question">{job.question}</p>
  {:else}
    <p class="said">{idle ? idle.said : now(job)}</p>
    {#if idle}
      {#if idle.ended}<p class="ended">{idle.ended}</p>{/if}
      {#if idle.run}
        <p class="run">To start an agent, on the machine that runs Iris, run <code>{idle.run}</code>. For another agent, put its name after <code>--agent</code>: <code>claude-code</code>, <code>codex</code>, or a name from <code>agents.toml</code>.</p>
      {/if}
    {/if}
    {#if mail}<p class="mail"><a class="touch" href={mail.link}>{mail.subject}</a></p>{/if}
  {/if}
</section>

<Grants {job} />

<section aria-labelledby="steps">
  <h2 id="steps"><span>Each step</span> <span class="count">{list.length}</span></h2>
  <ol class="steps">
    {#each list as s, i (s.key)}
      <li class="step {s.tone ?? ''}" class:last={i === list.length - 1}>
        <time class="time" datetime={s.at}>{clock(s.at)}</time>
        <span class="mark {s.who}" aria-hidden="true"></span>
        <div class="body">
          {#if newDay(i)}<p class="day">{day(s.at)}</p>{/if}
          <p class="text">{s.text}{#if s.link}{" "}<a href={s.link.href}>{s.link.text}</a>{/if}</p>
          {#if s.mail}<p class="mail"><a class="touch" href={s.mail.link}>{s.mail.subject}</a></p>{/if}
          {#if s.quote}<p class="quote">{s.quote}</p>{/if}
        </div>
      </li>
    {/each}
  </ol>
</section>

<style>
  .eyebrow {
    margin: var(--space-4) 0 var(--space-1);
    color: var(--color-text-faint);
    font-size: var(--font-size--1);
  }
  h1 {
    font-size: var(--font-size-3);
    font-weight: 500;
    letter-spacing: -0.01em;
    line-height: 1.25;
    margin: 0 0 var(--space-2);
  }
  .limits {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1) var(--space-4);
    margin: 0 0 var(--space-8);
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  .dot {
    display: inline-block;
    width: 8px;
    height: 8px;
    margin-left: var(--space-2);
    border-radius: var(--radius-full);
    background: var(--color-warning);
  }
  .late .dot {
    background: var(--color-error);
  }
  .now {
    margin: 0 0 var(--space-12);
    padding: var(--space-4) var(--space-6);
    background: var(--color-background-elevated);
    border: 1px solid var(--color-border-subtle);
    border-left: 4px solid var(--color-border);
    border-radius: var(--radius-lg);
  }
  .now.needs_you,
  .now.working {
    border-left-color: var(--color-primary);
  }
  .now.done {
    border-left-color: var(--color-success);
  }
  /* No agent works on the job: the border does not say "works". */
  .now.working.idle {
    border-left-color: var(--color-border);
  }
  p {
    margin: 0;
  }
  .label {
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  .question {
    font-size: var(--font-size-1);
    margin-top: var(--space-1);
    white-space: pre-wrap;
  }
  .said {
    font-size: var(--font-size-1);
  }
  .ended,
  .run {
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
    margin-top: var(--space-1);
  }
  code {
    font-family: var(--font-family-mono);
    overflow-wrap: anywhere;
  }
  .mail {
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
    margin-top: var(--space-1);
  }
  .mail a {
    color: var(--color-text-muted);
  }
  h2 {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    font-size: var(--font-size-0);
    font-weight: 600;
    margin: 0 0 var(--space-4);
    padding-bottom: var(--space-2);
    border-bottom: 1px solid var(--color-border);
  }
  .count {
    color: var(--color-text-muted);
    font-weight: 400;
    font-variant-numeric: tabular-nums;
  }
  .steps {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .step {
    display: grid;
    grid-template-columns: 48px 16px 1fr;
    column-gap: var(--space-3);
    position: relative;
  }
  /* The rail: one quiet line through the marks. */
  .step::before {
    content: "";
    position: absolute;
    left: calc(48px + var(--space-3) + 7px);
    top: 0;
    bottom: 0;
    width: 2px;
    background: var(--color-border-subtle);
  }
  .step:first-child::before {
    top: 12px;
  }
  .step:last-child::before {
    bottom: calc(100% - 12px);
  }
  .time {
    padding-top: 2px;
    color: var(--color-text-faint);
    font-size: var(--font-size--1);
    font-variant-numeric: tabular-nums;
    text-align: right;
  }
  .mark {
    position: relative;
    z-index: 1;
    width: 8px;
    height: 8px;
    margin: 8px 4px 0;
    border-radius: var(--radius-full);
    background: var(--color-background);
    border: 2px solid var(--color-text-faint);
    box-sizing: border-box;
  }
  .mark.you {
    background: var(--color-text-faint);
  }
  .mark.iris {
    border-color: var(--color-primary);
  }
  .ask .mark,
  .last .mark {
    border-color: var(--color-primary);
    background: var(--color-primary);
  }
  .done .mark {
    border-color: var(--color-success);
    background: var(--color-success);
  }
  .body {
    padding-bottom: var(--space-6);
    min-width: 0;
  }
  .day {
    color: var(--color-text-faint);
    font-size: var(--font-size--1);
    margin-bottom: var(--space-1);
  }
  .text a {
    color: var(--color-primary);
  }
  .quote {
    margin-top: var(--space-2);
    padding-left: var(--space-3);
    border-left: 2px solid var(--color-border);
    color: var(--color-text-muted);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .ask .quote {
    border-left-color: var(--color-primary);
    color: var(--color-text);
  }
  @media (max-width: 600px) {
    .step {
      grid-template-columns: 40px 16px 1fr;
      column-gap: var(--space-2);
    }
    .step::before {
      left: calc(40px + var(--space-2) + 7px);
    }
    .now {
      padding: var(--space-4);
    }
  }
</style>
