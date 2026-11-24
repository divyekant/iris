<!--
  The comments of the person on a changeset of an agent or a local author
  (card R4-08, spec docs/specs/2026-11-17-review-comments.md, decision D15).
  A job agent reads the comment in its next session and submits a new
  revision. Another author reads it in the changeset document. Only an open
  changeset takes a comment. The list stays.
-->
<script lang="ts">
  import { get, post, type Changeset } from "../api";
  import { short } from "../review";

  type Comment = { id: number; changeset: string; revision: number; text: string; at: string };

  // `refused` loads the request again after a 409: the request changed after the page loaded it.
  // `job`: a job agent reads the comment in its next session; another author reads it with the request (decision D15).
  let { changeset: c, agent, job = true, refused = () => {} }: { changeset: Changeset; agent: string; job?: boolean; refused?: (why: string) => unknown } = $props();
  // The box opens from one quiet button (option B, the owner's pick).
  let writing = $state(false);

  // The engine keeps 1 to 4,000 characters (spec section 3).
  const MAX = 4000;
  let path = $derived(`changesets/${encodeURIComponent(c.id)}/comments`);
  let list: Comment[] = $state([]);
  let text = $state("");
  let busy = $state(false);
  let problem = $state("");
  let saved = $state("");
  // One key for one text: a send again of the same text after an unknown result makes no second comment.
  let key = "";
  let keyed = "";

  const words = (e: unknown) => (e instanceof Error ? e.message : String(e));
  // Join by ID: a load that answers after a send must not hide the new comment.
  function keep(more: Comment[]) {
    list = [...new Map([...list, ...more].map((x) => [x.id, x])).values()].sort((a, b) => a.id - b.id);
  }
  function load() {
    get<{ comments: Comment[] }>(path).then(
      (got) => keep(got.comments),
      (e) => (problem = `Iris could not load the comments. ${words(e)}`),
    );
  }
  $effect(() => {
    void path;
    load();
  });

  // The text goes as written. The engine counts characters (code points), not UTF-16 units.
  let count = $derived([...text].length);
  let ready = $derived(!busy && text.trim() !== "" && count <= MAX && c.state === "open");

  async function send(e: SubmitEvent) {
    e.preventDefault();
    if (!ready) return;
    const body = text;
    // A key names one text on one revision: a new revision after a 409 gets a new key.
    const sig = `${c.revision}\n${body}`;
    if (sig !== keyed) {
      keyed = sig;
      key = globalThis.crypto.randomUUID();
    }
    busy = true;
    problem = saved = "";
    try {
      const got = await post<Comment>(path, { revision: c.revision, text: body, key });
      keep([got]);
      text = keyed = "";
      saved = `Iris gave your comment to ${agent}. The agent reads it in its next session and sends a new revision.`;
    } catch (err) {
      const status = (err as { status?: number }).status;
      if (status === 409) {
        keyed = "";
        // Busy until the request loads again, so that the next send names the new revision.
        await refused(`Iris did not save your comment: ${words(err)} Here is the request as it is now.`);
      }
      // With no answer or a 5xx, the result is unknown. The same text sends the same key, so Iris saves it once.
      else if (status === undefined || status >= 500) {
        problem = "Iris does not know if it saved your comment. Send it again: Iris keeps it only once.";
        // Spec section 4: after a lost answer, load the comments again. A saved comment then shows.
        load();
      }
      else problem = words(err);
    } finally {
      busy = false;
    }
  }
</script>

<!-- A request that takes no new comment shows the part only when it has comments. -->
{#if list.length || c.state === "open"}
<section class="comments" aria-labelledby={`${c.id}-comments`}>
  <h3 id={`${c.id}-comments`}>Comments to {agent}</h3>
  {#if list.length}
    <ol>
      {#each list as m (m.id)}
        <li>
          <p class="when">You, on revision {m.revision} <span class="time">{short(m.at)}</span></p>
          <p class="said">{m.text}</p>
        </li>
      {/each}
    </ol>
  {/if}
  {#if c.state === "open" && !writing}
    <button type="button" class="quiet" onclick={() => (writing = true)}>Ask {agent} for a change</button>
  {:else if c.state === "open"}
    <form onsubmit={send}>
      <label class="hint" for={`${c.id}-comment`}>Tell {agent} what to change. {job ? "The agent sends a new revision of this request." : `${agent} reads it the next time it looks at this request.`}</label>
      <textarea id={`${c.id}-comment`} rows="3" bind:value={text} disabled={busy}></textarea>
      <div class="row">
        <button type="submit" class="outline" disabled={!ready}>Send to {agent}</button>
        {#if count > MAX - 200}<span class="count">{count} of {MAX}</span>{/if}
      </div>
    </form>
  {/if}
  {#if problem}<p role="alert">{problem}</p>{/if}
  {#if saved}<p class="saved" role="status">{saved}</p>{/if}
</section>
{/if}

<style>
  .comments {
    margin: var(--space-4) 0;
    padding-top: var(--space-4);
    border-top: 1px solid var(--color-border-subtle);
  }
  h3 {
    margin: 0 0 var(--space-2);
    font-size: var(--font-size-0);
    font-weight: 600;
  }
  ol {
    list-style: none;
    margin: 0 0 var(--space-3);
    padding: 0;
  }
  li {
    padding: var(--space-2) 0 var(--space-2) var(--space-4);
    border-left: 2px solid var(--color-border);
  }
  .when,
  .hint,
  .count {
    margin: 0;
    color: var(--color-text-faint);
    font-size: var(--font-size--1);
  }
  .time {
    margin-left: var(--space-1);
  }
  .said {
    margin: var(--space-1) 0 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-width: 70ch;
  }
  .hint {
    display: block;
    margin-bottom: var(--space-2);
  }
  textarea {
    display: block;
    width: 100%;
    box-sizing: border-box;
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    background: var(--color-background-elevated);
    color: var(--color-text);
    font: inherit;
    line-height: 1.4;
    resize: vertical;
  }
  textarea:focus {
    outline: none;
    border-color: var(--color-primary);
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
    margin-top: var(--space-2);
  }
  .outline {
    padding: var(--space-2) var(--space-4);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-full);
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-weight: 500;
    cursor: pointer;
    transition: border-color var(--transition-fast);
  }
  .outline:hover:not(:disabled),
  .outline:focus-visible {
    border-color: var(--color-text-muted);
  }
  .outline:focus-visible {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }
  .quiet {
    padding: 0;
    border: 0;
    background: none;
    color: var(--color-text-muted);
    font: inherit;
    font-size: var(--font-size--1);
    text-decoration: underline;
    text-underline-offset: 3px;
    cursor: pointer;
  }
  .quiet:hover {
    color: var(--color-text);
  }
  .outline:disabled {
    cursor: default;
    opacity: 0.6;
  }
  [role="alert"],
  .saved {
    margin: var(--space-2) 0 0;
    color: var(--color-text-muted);
  }
  @media (max-width: 600px), (any-pointer: coarse) {
    .outline {
      min-height: var(--size-touch);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .outline {
      transition: none;
    }
  }
</style>
