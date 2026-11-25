<!--
  One job of an agent (card R4-09): the goal, its limits, the state now, and
  each step in order: the moves of the job and the changesets of the agent.
  The words come from jobs.ts. Text of the agent and of mail is data, and
  shows as text only.
-->
<script lang="ts">
  import { own, type JobDetail } from "../api";
  import { agent, asker, clock, day, due, editJob, endJob, loadJob, now, sameDue, steps, waitMail, wakeJob } from "../jobs";
  import AnswerForm from "./AnswerForm.svelte";
  import Grants from "./Grants.svelte";
  import JobForm from "./JobForm.svelte";
  import Scope from "./Scope.svelte";

  // `every`: the ms between two reads of a job whose session can still change (a prop for the tests).
  let { detail: first, every = 5000 }: { detail: JobDetail; every?: number } = $props();
  // svelte-ignore state_referenced_locally
  let detail = $state(first);
  // Each request of this page about the job (a read, an edit, an end, an answer, a scope change) runs
  // in one queue, one at a time, and a job that it gives shows before the next request starts. So the page
  // shows the job of the last request, and a late answer never puts back an older job (issue 322).
  let chain: Promise<unknown> = Promise.resolve();
  let queued = 0;
  function queue<T>(work: () => Promise<T>): Promise<T> {
    queued++;
    const step = chain.then(async () => {
      try {
        const out = await work();
        if (isDetail(out)) detail = out;
        return out;
      } finally {
        queued--;
      }
    });
    chain = step.catch(() => {});
    return step;
  }
  const isDetail = (x: unknown): x is JobDetail => typeof (x as JobDetail)?.job?.id === "string";
  // Read the job again and show it. It throws if the read fails.
  const reload = () => queue(() => loadJob(detail.job.id));
  // A failed read counts here, so the next read is planned again.
  let misses = $state(0);
  // While the job works, its session can start, fail or end at any time, also
  // from `iris job run` at the terminal: read the job again (issue 742).
  $effect(() => {
    void misses;
    if (detail.job.state !== "working") return;
    let live = true;
    const timer = setTimeout(() => {
      // A request of the person runs: its answer shows the job, and the next poll reads after it.
      if (queued > 0) return void misses++;
      reload().catch(() => {
        if (live) misses++;
      });
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

  let editing = $state(false);
  async function save(goal: string, dueAt: string | null): Promise<string> {
    try {
      await queue(() => editJob(job.id, goal, dueAt));
      editing = false;
      return "";
    } catch (e) {
      // The edit can have arrived (a lost answer, or a failed read after it): show the job as the engine has it.
      const fresh = await reload().catch(() => null);
      if (fresh) {
        if (fresh.job.goal === goal && sameDue(fresh.job.due, dueAt)) {
          editing = false;
          return "";
        }
      }
      return own(e) || (e as Error).message;
    }
  }

  // End the job (issue 322): one question in place, as the remove of an account.
  let ending = $state(false);
  let endBusy = $state(false);
  let endError = $state("");
  async function end(kind: "complete" | "cancel") {
    endBusy = true;
    endError = "";
    try {
      await queue(() => endJob(job.id, kind));
    } catch (e) {
      const status = (e as { status?: number }).status;
      // A refusal (4xx, not a conflict) wrote nothing. Any other failure can come after the end: no answer,
      // a body that the client cannot read, a 5xx, or a 409 because the job ended already. Read the job again.
      if (status !== undefined && status >= 400 && status < 500 && status !== 409) endError = own(e) || (e as Error).message;
      else await reload().catch(() => {});
    } finally {
      endBusy = false;
    }
    if (detail.job.state === "done") ending = editing = false;
    else endError ||= "Iris does not know if the job ended. Try again, or open the job again later.";
  }


  // End the wait early (issue 959). A wake by the person starts no agent: the Now card then gives the command.
  let wakeBusy = $state(false);
  let wakeError = $state("");
  async function wake() {
    wakeBusy = true;
    wakeError = "";
    try {
      await queue(() => wakeJob(job));
    } catch (e) {
      // A 409 wrote nothing: the job does not wait now, or waits for another thing. Any failure with
      // no answer can come after the wake. Both read the job again and show it.
      wakeError = own(e) || (e as Error).message;
      await reload().catch(() => {});
      if (detail.job.state !== "waiting" && (e as { status?: number }).status === undefined) wakeError = "";
    } finally {
      wakeBusy = false;
    }
  }
</script>

<p class="eyebrow">Job</p>
{#if editing}
  <JobForm goal={job.goal} due={job.due} action="Save" {save} oncancel={() => (editing = false)} />
{:else}
  <h1>{job.goal}</h1>
  <p class="limits">
    {#if limit}<span class={limit.tone}>{limit.text}{#if limit.tone}<span class="dot" aria-hidden="true"></span>{/if}</span>{/if}
    <span>Started {day(job.created)}</span>
    {#if job.state !== "done"}
      <button class="edit" type="button" onclick={() => ((editing = true), (ending = false))}>Edit</button>
      <button class="edit" type="button" onclick={() => ((ending = true), (endError = ""))} disabled={ending}>End</button>
    {/if}
  </p>
{/if}

{#if ending && job.state !== "done"}
  <div class="ask" role="group" aria-label="End the job">
    <p>End this job? The agent stops, and the copy of the mail for the agent goes. The steps stay on this page.</p>
    <p class="actions">
      <button type="button" class="primary" onclick={() => end("complete")} disabled={endBusy}>Mark as done</button>
      <button type="button" class="quiet" onclick={() => end("cancel")} disabled={endBusy}>Cancel the job</button>
      <button type="button" class="quiet" onclick={() => ((ending = false), (endError = ""))} disabled={endBusy}>Keep the job</button>
    </p>
    {#if endError}<p class="error" role="alert">{endError}</p>{/if}
  </div>
{/if}

<section class="now {job.state}" class:idle aria-label="Now">
  {#if job.state === "needs_you"}
    <p class="label">{asker(job)} asks you</p>
    <p class="question">{job.question}</p>
    {#key job.updated}
      <AnswerForm
        {job}
        {queue}
        onanswered={() => {}}
        onchanged={() => reload().catch(() => {})}
        onunknown={() => reload().catch(() => {})}
        lost="Iris does not know if your answer arrived. Open the job again to see."
      />
    {/key}
  {:else}
    <p class="said">{idle ? idle.said : now(job)}</p>
    {#if idle}
      {#if idle.ended}<p class="ended">{idle.ended}</p>{/if}
      {#if idle.run}
        <p class="run">To start an agent, on the machine that runs Iris, run <code>{idle.run}</code>. For another agent, put its name after <code>--agent</code>: <code>claude-code</code>, <code>codex</code>, or a name from <code>agents.toml</code>.</p>
      {/if}
    {/if}
    {#if mail}<p class="mail"><a class="touch" href={mail.link}>{mail.subject}</a></p>{/if}
    {#if job.state === "waiting"}
      <p class="wake"><button class="edit" type="button" onclick={wake} disabled={wakeBusy}>Wake now</button></p>
    {/if}
    {#if wakeError}<p class="error" role="alert">{wakeError}</p>{/if}
  {/if}
</section>

<!-- The queue shows the job of each scope request, so `onchange` has nothing to do here. -->
<Scope {detail} {queue} onchange={() => {}} />

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
  .edit {
    font: inherit;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--color-primary);
    cursor: pointer;
  }
  .edit:hover {
    text-decoration: underline;
  }
  .edit:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }
  .edit:disabled {
    color: var(--color-text-faint);
    cursor: default;
    text-decoration: none;
  }
  .ask {
    margin: calc(-1 * var(--space-4)) 0 var(--space-8);
    padding: var(--space-4);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-3);
    margin-top: var(--space-3);
  }
  .actions button {
    padding: var(--space-2) var(--space-4);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-full);
    background: transparent;
    color: var(--color-text);
    font: inherit;
    cursor: pointer;
    transition: border-color var(--transition-fast);
  }
  .actions button:hover:not(:disabled) {
    border-color: var(--color-text-muted);
  }
  .actions .primary {
    border-color: var(--color-primary);
    background: var(--color-primary);
    color: var(--color-on-primary);
    font-weight: 500;
  }
  .actions button:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .actions button:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }
  .ask .error,
  .now .error {
    margin-top: var(--space-3);
    border-left: 4px solid var(--color-error);
    padding-left: var(--space-2);
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
  .wake {
    margin-top: var(--space-2);
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
  @media (max-width: 600px), (any-pointer: coarse) {
    .actions button,
    .edit {
      min-height: var(--size-touch);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .actions button {
      transition: none;
    }
  }
</style>
