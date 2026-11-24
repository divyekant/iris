<!--
  The watches that wait (card R5-07), and the field that starts a new watch. A
  ring without a center marks a watch that waits. The Arrived card fills the ring.
-->
<script lang="ts">
  import { wrote } from "../api";
  import { addWatch, day, removeWatch, type Watch } from "../watches";

  // `onadded` gets the new watch, or null when the engine gave none (an older engine).
  let { watches, onadded, onstopped }: { watches: Watch[]; onadded: (w: Watch | null) => void; onstopped: (id: string) => void } = $props();

  let adding = $state(false);
  let text = $state("");
  let busy = $state(false);
  let error = $state("");
  let input: HTMLInputElement | undefined = $state();

  $effect(() => {
    if (adding) input?.focus();
  });

  async function add(event: SubmitEvent) {
    event.preventDefault();
    if (!text.trim() || busy) return;
    busy = true;
    error = "";
    try {
      const w = await addWatch(text);
      text = "";
      adding = false;
      onadded(w?.id ? w : null);
    } catch (e) {
      error = wrote(e, "started the watch", "Iris could not start the watch. Try again.");
    } finally {
      busy = false;
    }
  }

  async function stop(id: string) {
    busy = true;
    error = "";
    try {
      await removeWatch(id);
      onstopped(id);
    } catch (e) {
      if ((e as { status?: number }).status === 404) onstopped(id);
      else error = wrote(e, "stopped the watch", "Iris could not stop the watch. Try again.");
    } finally {
      busy = false;
    }
  }
</script>

{#snippet ring()}
  <svg class="ring" viewBox="0 0 24 24" width="20" height="20" aria-hidden="true">
    <circle cx="12" cy="12" r="8" fill="none" stroke="var(--color-primary)" stroke-width="2" />
  </svg>
{/snippet}

{#if watches.length}
  <ul class="watches">
    {#each watches as w (w.id)}
      <li>
        {@render ring()}
        <span class="text">{w.text}</span>
        <span class="since">since {day(w.created)}</span>
        <button type="button" class="stop" onclick={() => stop(w.id)} disabled={busy} aria-label={`Stop the watch for ${w.text}`}>Stop</button>
      </li>
    {/each}
  </ul>
{/if}

{#if adding}
  <form onsubmit={add}>
    <label for="watch-text">Tell me when a new mail matches</label>
    <div class="field">
      <input id="watch-text" onkeydown={(e) => e.key === "Escape" && ((adding = false), (error = ""))} bind:this={input} bind:value={text} placeholder="visa from:consulate" maxlength="500" autocomplete="off" spellcheck="false" />
      <button type="submit" disabled={busy || !text.trim()}>Watch</button>
    </div>
    <p class="hint">The words work as in search: words, "a phrase", from:, to:, label: and form:. Iris tells you one time.</p>
  </form>
{:else}
  <button type="button" class="start" onclick={() => (adding = true)}>
    {@render ring()}
    {watches.length ? "Watch for another mail" : "Watch for a mail"}
  </button>
{/if}
{#if error}<p class="error" role="alert">{error}</p>{/if}

<style>
  .watches {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  li {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-2) 0;
    border-bottom: 1px solid var(--color-border-subtle);
  }
  .ring {
    flex: none;
  }
  .text {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .since {
    flex: none;
    color: var(--color-text-faint);
    font-size: var(--font-size--1);
  }
  .stop {
    flex: none;
    padding: 0 var(--space-1);
    border: none;
    background: none;
    color: var(--color-text-muted);
    font: inherit;
    font-size: var(--font-size--1);
    text-decoration: underline;
    text-underline-offset: 3px;
    cursor: pointer;
    opacity: 0;
    transition: opacity var(--transition-fast);
  }
  li:hover .stop,
  .stop:focus-visible {
    opacity: 1;
  }
  @media (hover: none), (any-pointer: coarse) {
    .stop {
      opacity: 1;
    }
  }
  .start {
    display: inline-flex;
    align-items: center;
    gap: var(--space-2);
    margin-top: var(--space-3);
    padding: var(--space-1) var(--space-3) var(--space-1) var(--space-2);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-full);
    background: transparent;
    color: var(--color-text-muted);
    font: inherit;
    font-size: var(--font-size--1);
    cursor: pointer;
    transition: border-color var(--transition-fast), color var(--transition-fast);
  }
  .start:hover {
    border-color: var(--color-primary);
    color: var(--color-text);
  }
  form {
    margin-top: var(--space-4);
  }
  label {
    display: block;
    margin-bottom: var(--space-2);
    font-weight: 500;
  }
  .field {
    display: flex;
    gap: var(--space-2);
  }
  input {
    flex: 1;
    min-width: 0;
    padding: var(--space-2) var(--space-4);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-full);
    background: var(--color-background-elevated);
    color: var(--color-text);
    font: inherit;
  }
  input:focus-visible {
    outline: none;
    border-color: var(--color-primary);
  }
  input::placeholder {
    color: var(--color-text-faint);
  }
  .field button {
    flex: none;
    padding: var(--space-2) var(--space-4);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-full);
    background: transparent;
    color: var(--color-text);
    font: inherit;
    cursor: pointer;
  }
  .field button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .hint {
    margin: var(--space-2) 0 0;
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  .error {
    margin: var(--space-2) 0 0;
    border-left: 4px solid var(--color-error);
    padding-left: var(--space-2);
    font-size: var(--font-size--1);
  }
  @media (prefers-reduced-motion: reduce) {
    .stop,
    .start {
      transition: none;
    }
  }
</style>
