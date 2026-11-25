<!--
  The answer field of a job question (card R5-05): the brief shows it in the
  card of each question, and the job page in its Now card (issue 322). The
  question is text of the agent: the views show it as text only.
-->
<script lang="ts">
  import { own, type Job, type JobDetail } from "../api";
  import { answerJob } from "../jobs";

  let {
    job,
    draft = "",
    ondraft = () => {},
    onanswered,
    onchanged,
    onsend = () => {},
    onbusy = () => {},
    onunknown = () => {},
    lost = "Iris does not know if your answer arrived. Open the job to see.",
  }: {
    job: Job;
    draft?: string;
    ondraft?: (text: string) => void;
    onanswered: (job: Job, detail: JobDetail) => void;
    onchanged: () => void;
    /** Called when the answer request starts. */
    onsend?: () => void;
    /** True when the answer request starts, false when it ends (before any other callback). */
    onbusy?: (on: boolean) => void;
    /** Called when Iris does not know if the answer arrived. */
    onunknown?: () => void;
    lost?: string;
  } = $props();

  // svelte-ignore state_referenced_locally
  let text = $state(draft);
  let busy = $state(false);
  let error = $state("");
  let empty = $derived(!text.trim());

  async function send(event?: SubmitEvent) {
    event?.preventDefault();
    if (empty || busy) return;
    // The engine counts characters (code points), not the UTF-16 units of `maxlength`.
    if ([...text].length > 10_000) {
      error = "Your answer is longer than 10,000 characters. Make it shorter.";
      return;
    }
    busy = true;
    error = "";
    try {
      onsend();
      onbusy(true);
      let detail: JobDetail;
      try {
        detail = await answerJob(job, text);
      } finally {
        onbusy(false);
      }
      onanswered(job, detail);
    } catch (e) {
      const status = (e as { status?: number }).status;
      if (status === 409 || status === 404) {
        // Another tab answered, the agent asked again, or the person ended the job: the view loads the job again.
        error = "This job does not ask this question now.";
        onchanged();
      } else if (status === undefined || status >= 500) {
        // No answer, a body that the client cannot read, or a 5xx: the engine can have stored it, so say
        // that the result is not known, never the access text (issue 610).
        error = lost;
        onunknown();
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

<form onsubmit={send}>
  <label class="hidden" for={`answer-${job.id}`}>Your answer</label>
  <textarea id={`answer-${job.id}`} bind:value={text} oninput={(e) => ondraft(e.currentTarget.value)} onkeydown={keys} rows="2" placeholder="Your answer" disabled={busy}></textarea>
  <div class="row">
    <button type="submit" disabled={empty || busy}>{busy ? "Sending" : "Answer"}</button>
    <span class="hint">The job goes on with your answer.</span>
  </div>
</form>
{#if error}<p class="error" role="alert">{error}</p>{/if}

<style>
  p {
    margin: 0;
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
  @media (max-width: 600px), (any-pointer: coarse) {
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
