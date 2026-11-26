<!--
  The question of a job in the brief (card R5-05; from the agent, or from Iris
  on the due date): the goal of the job, the question, and one field for your
  answer. The job goes on with the answer: a job that had a session gets a new
  one, and a job with no session starts none. The brief then says so and loads
  the questions again. The question is text of the agent: it shows as text only.
-->
<script lang="ts">
  import type { Job, JobDetail } from "../api";
  import { href } from "../route";
  import AnswerForm from "./AnswerForm.svelte";

  let {
    job,
    draft = "",
    ondraft = () => {},
    onanswered,
    onchanged,
  }: { job: Job; draft?: string; ondraft?: (text: string) => void; onanswered: (job: Job, detail: JobDetail) => void; onchanged: () => void } = $props();
</script>

<article class="question" aria-label="A question for you">
  <p class="for"><span class="mark" aria-hidden="true"></span>A question for you, from the job <a href={href.job(job.id)}>{job.goal}</a></p>
  <p class="text">{job.question}</p>
  <AnswerForm {job} {draft} {ondraft} {onanswered} {onchanged} />
</article>

<style>
  .question {
    padding: var(--space-4);
    border: 1px solid var(--color-border-subtle);
    border-left: 4px solid var(--color-primary);
    border-radius: var(--radius-lg);
    background: var(--color-background-elevated);
  }
  p {
    margin: 0;
  }
  .for {
    display: flex;
    align-items: baseline;
    flex-wrap: wrap;
    gap: 0 var(--space-2);
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
    overflow-wrap: anywhere;
  }
  .for a {
    color: var(--color-text-muted);
  }
  .for a:hover {
    color: var(--color-text);
  }
  /* The mark of a job that asks you, as in the Jobs view: a primary dot. */
  .mark {
    flex: none;
    width: var(--space-2);
    height: var(--space-2);
    border-radius: var(--radius-full);
    background: var(--color-primary);
  }
  .text {
    margin-top: var(--space-2);
    font-size: var(--font-size-1);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
</style>
