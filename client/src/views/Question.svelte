<!--
  The question of an agent in the brief (card R5-05): the goal of the job, the
  question, and one field for your answer. The answer starts a new session of
  the job. The question is text of the agent: it shows as text only.
-->
<script lang="ts">
  import { own, type Job } from "../api";
  import { answerJob } from "../jobs";
  import { href } from "../route";

  let { job, onchanged }: { job: Job; onchanged: () => void } = $props();

  let text = $state("");
  let busy = $state(false);
  let sent = $state(false);
  let error = $state("");
  let empty = $derived(!text.trim());

  async function send(event?: SubmitEvent) {
    event?.preventDefault();
    if (empty || busy) return;
    busy = true;
    error = "";
    try {
      await answerJob(job.id, text);
      sent = true;
    } catch (e) {
      const status = (e as { status?: number }).status;
      if (status === 409 || status === 404) {
        // Another tab answered, or the person ended the job: the brief loads the questions again.
        error = "This job asks no question now.";
        onchanged();
      } else if (status === undefined) {
        error = own(e) || "Iris does not know if your answer arrived. Open the job to see.";
      } else {
        error = own(e) || "Iris could not send your answer. Try again.";
      }
    } finally {
      busy = false;
    }
  }

  // Ctrl or Command with Enter sends. Enter alone starts a new line.
  const keys = (e: KeyboardEvent) => e.key === "Enter" && (e.metaKey || e.ctrlKey) && (e.preventDefault(), send());
</script>

<article class="question" aria-label="An agent asks you">
  <p class="for"><span class="mark" aria-hidden="true"></span>An agent asks you, for the job <a href={href.job(job.id)}>{job.goal}</a></p>
  <p class="text">{job.question}</p>
  {#if sent}
    <p class="sent" role="status">
      <svg viewBox="0 0 16 16" width="16" height="16" aria-hidden="true"><path d="M3 8.5l3 3 7-7" fill="none" stroke="var(--color-primary)" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" /></svg>
      Sent. The agent continues with your answer.
    </p>
  {:else}
    <form onsubmit={send}>
      <label class="hidden" for={`answer-${job.id}`}>Your answer</label>
      <textarea id={`answer-${job.id}`} bind:value={text} onkeydown={keys} rows="2" maxlength="10000" placeholder="Your answer" disabled={busy}></textarea>
      <div class="row">
        <button type="submit" disabled={empty || busy}>{busy ? "Sending" : "Answer"}</button>
        <span class="hint">The agent reads it in a new session.</span>
      </div>
    </form>
  {/if}
  {#if error}<p class="error" role="alert">{error}</p>{/if}
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
  form {
    margin-top: var(--space-4);
  }
  textarea {
    display: block;
    box-sizing: border-box;
    width: 100%;
    min-height: calc(var(--space-12) + var(--space-4));
    field-sizing: content;
    max-height: 40vh;
    padding: var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-background);
    color: var(--color-text);
    font: inherit;
    resize: vertical;
    transition: border-color var(--transition-fast);
  }
  textarea:focus {
    outline: none;
    border-color: var(--color-primary);
  }
  .row {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-3);
    margin-top: var(--space-3);
  }
  button {
    padding: var(--space-2) var(--space-4);
    border: 1px solid var(--color-primary);
    border-radius: var(--radius-full);
    background: var(--color-primary);
    color: var(--color-on-primary);
    font: inherit;
    font-weight: 500;
    cursor: pointer;
    transition: opacity var(--transition-fast);
  }
  button:disabled {
    background: transparent;
    color: var(--color-text-muted);
    border-color: var(--color-border);
    cursor: default;
  }
  .hint {
    color: var(--color-text-faint);
    font-size: var(--font-size--1);
  }
  .sent {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    margin-top: var(--space-3);
    color: var(--color-text-muted);
  }
  .error {
    margin-top: var(--space-3);
    border-left: 4px solid var(--color-error);
    padding-left: var(--space-2);
    font-size: var(--font-size--1);
  }
  .hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }
  @media (max-width: 600px), (pointer: coarse) {
    button {
      min-height: var(--size-touch);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    textarea,
    button {
      transition: none;
    }
  }
</style>
