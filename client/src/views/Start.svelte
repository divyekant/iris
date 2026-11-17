<!--
  "Handle this": the person gives a goal to an agent, for one commitment or
  conversation (card R4-10). The form shows each message that the job will
  read, so the grant is what the person saw. The goal is the words of the
  person: no text of the mail fills it.
-->
<script lang="ts">
  import { get, post, type Item, type Job } from "../api";
  import { when } from "../format";
  import { href } from "../route";
  import { byThread, dueDate, dueValue, reads, type Preview } from "../start";

  let { item, oncancel, onitem }: { item: Item; oncancel?: () => void; onitem?: (item: Item) => void } = $props();

  // svelte-ignore state_referenced_locally
  let due = $state(dueDate(item));
  let goal = $state("");
  let preview = $state<Preview | null>(null);
  let problem = $state("");
  let note = $state("");
  let sending = $state(false);

  async function load() {
    problem = "";
    try {
      preview = await get<Preview>(`items/${encodeURIComponent(item.id)}/job`);
    } catch (error) {
      preview = null;
      problem = (error as Error).message;
    }
  }
  $effect(() => {
    load();
  });
  async function reload() {
    try {
      onitem?.(await get<Item>(`items/${encodeURIComponent(item.id)}`));
    } catch {
      // The note says why the start failed. The page keeps the item that it has.
    }
  }

  let groups = $derived(preview ? byThread(preview.messages) : []);
  let ready = $derived(goal.trim().length > 0 && preview !== null && !sending);

  async function start(event: SubmitEvent) {
    event.preventDefault();
    if (!ready || !preview) return;
    sending = true;
    note = "";
    try {
      const job = await post<Job>("jobs", {
        item: item.id,
        goal: goal.trim(),
        due: dueValue(item, due),
        messages: preview.messages.map((m) => m.id),
      });
      location.hash = href.job(job.id);
    } catch (error) {
      const e = error as Error & { status?: number; body?: { job?: string } };
      if (e.status === 409 && e.body?.job) {
        location.hash = href.job(e.body.job);
        return;
      }
      note = e.message;
      // The item or its mail changed after the form loaded them: show both as they are now.
      if (e.status === 409) await Promise.all([reload(), load()]);
    } finally {
      sending = false;
    }
  }
</script>

<form class="start" onsubmit={start}>
  <label class="goal">
    <span class="label">Goal</span>
    <!-- svelte-ignore a11y_autofocus -->
    <textarea bind:value={goal} rows="3" maxlength="2000" placeholder="What should the agent get done? For example: Send Dana my comments on section 4." autofocus></textarea>
  </label>

  <div class="due">
    <label>
      <span class="label">Due</span>
      <input type="date" bind:value={due} />
    </label>
    {#if due}<button class="quiet" type="button" onclick={() => (due = "")}>No due date</button>{/if}
  </div>

  <div class="reads">
    <p class="label">The agent reads {preview ? reads(preview.messages) : "…"}, and no other mail.</p>
    {#if problem}
      <p class="error">{problem}</p>
    {:else}
      {#each groups as g (g.thread)}
        <div class="thread">
          <p class="subject">{g.subject}</p>
          <ul>
            {#each g.messages as m (m.id)}
              <li>
                <span class="who">{m.from.name ?? m.from.address ?? "Unknown sender"}</span>
                <span class="when">{m.date ? when(m.date) : ""}</span>
                {#if !m.readable}<span class="hidden">Hidden from the agent for now</span>{/if}
                <!-- A thread can hold more than one subject: each message that the job reads shows its own. -->
                {#if (m.subject ?? "(no subject)") !== g.subject}<span class="what">{m.subject ?? "(no subject)"}</span>{/if}
              </li>
            {/each}
          </ul>
        </div>
      {/each}
    {/if}
  </div>

  {#if note}<p class="error" role="alert">{note}</p>{/if}

  <div class="row">
    <button class="go" type="submit" disabled={!ready}>{sending ? "Starting…" : "Start the job"}</button>
    {#if oncancel}<button class="quiet" type="button" onclick={oncancel}>Cancel</button>{/if}
  </div>
  <p class="fine">The agent asks you before each change to your mail, unless you allow it on the job page.</p>
</form>

<style>
  .start {
    display: grid;
    gap: var(--space-6);
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
  .due {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-end;
    gap: var(--space-2) var(--space-4);
  }
  input {
    min-height: calc(var(--space-8) + var(--space-2));
    padding: 0 var(--space-3);
  }
  .thread {
    margin-top: var(--space-3);
  }
  .subject {
    font-weight: 500;
  }
  ul {
    list-style: none;
    margin: var(--space-1) 0 0;
    padding: 0 0 0 var(--space-4);
    border-left: 1px solid var(--color-border);
  }
  li {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-1) var(--space-3);
    padding: var(--space-1) 0;
    font-size: var(--font-size--1);
  }
  .when {
    color: var(--color-text-faint);
    font-variant-numeric: tabular-nums;
  }
  .hidden {
    color: var(--color-text-muted);
  }
  .what {
    flex-basis: 100%;
    color: var(--color-text-muted);
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2);
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
  .fine {
    color: var(--color-text-faint);
    font-size: var(--font-size--1);
  }
</style>
