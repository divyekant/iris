<!--
  The goal and the due date of a job, typed by the person (decision D25): a
  new job with no mail, or an edit of an open job. `save` gets the goal and
  the `due` value and gives the text of a failure, or "" when it worked.
-->
<script lang="ts">
  import { ymd } from "../act";
  import { keepDue } from "../start";

  let {
    goal: first = "",
    due: old,
    day,
    action,
    save,
    oncancel,
    ondraft,
  }: {
    goal?: string;
    due?: string;
    /** The day of the date field of a kept draft, as `YYYY-MM-DD`. */
    day?: string;
    action: string;
    save: (goal: string, due: string | null) => Promise<string>;
    oncancel: () => void;
    /** Gets the goal and the day of the date field after each change. */
    ondraft?: (goal: string, day: string) => void;
  } = $props();

  // svelte-ignore state_referenced_locally
  let goal = $state(first);
  // svelte-ignore state_referenced_locally
  let due = $state(day ?? (old ? ymd(new Date(old)) : ""));
  $effect(() => ondraft?.(goal, due));
  let note = $state("");
  let sending = $state(false);
  let ready = $derived(goal.trim().length > 0 && !sending);

  async function send(event: SubmitEvent) {
    event.preventDefault();
    if (!ready) return;
    sending = true;
    note = await save(goal.trim(), keepDue(old, due));
    sending = false;
  }
</script>

<form class="job-form" onsubmit={send}>
  <label>
    <span class="label">Goal</span>
    <!-- svelte-ignore a11y_autofocus -->
    <textarea bind:value={goal} rows="3" maxlength="2000" placeholder="What should the agent get done? For example: Book a dentist appointment for next week." autofocus></textarea>
  </label>
  <div class="due">
    <label>
      <span class="label">Due</span>
      <input type="date" bind:value={due} />
    </label>
    {#if due}<button class="quiet" type="button" onclick={() => (due = "")}>No due date</button>{/if}
  </div>
  {#if note}<p class="error" role="alert">{note}</p>{/if}
  <div class="row">
    <button class="go" type="submit" disabled={!ready}>{sending ? "Saving…" : action}</button>
    <button class="quiet" type="button" onclick={oncancel}>Cancel</button>
  </div>
</form>

<style>
  .job-form {
    display: grid;
    gap: var(--space-4);
    margin: 0 0 var(--space-8);
    padding: var(--space-6);
    background: var(--color-background-elevated);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-lg);
  }
  p {
    margin: 0;
  }
  .label {
    display: block;
    margin-bottom: var(--space-2);
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  textarea,
  input {
    font: inherit;
    color: var(--color-text);
    background: var(--color-background);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    color-scheme: inherit;
  }
  textarea {
    width: 100%;
    box-sizing: border-box;
    padding: var(--space-3) var(--space-4);
    font-size: var(--font-size-1);
    line-height: 1.4;
    resize: vertical;
  }
  textarea:focus,
  input:focus {
    outline: none;
    border-color: var(--color-primary);
  }
  input {
    min-height: calc(var(--space-8) + var(--space-2));
    padding: 0 var(--space-3);
  }
  .due,
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: var(--space-2) var(--space-4);
  }
  button {
    font: inherit;
    cursor: pointer;
    min-height: calc(var(--space-8) + var(--space-2));
    border-radius: var(--radius-full);
    transition: background var(--transition-fast), color var(--transition-fast);
  }
  .go {
    padding: 0 var(--space-6);
    border: 0;
    background: var(--color-primary);
    color: var(--color-on-primary);
    font-weight: 600;
  }
  .go:disabled {
    opacity: 0.5;
    cursor: default;
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
  button:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }
  .error {
    border-left: 4px solid var(--color-error);
    padding-left: var(--space-3);
  }
</style>
