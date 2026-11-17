<!--
  The one notice of each watch whose mail came (card R5-07). It shows at the top
  of the brief until the person says Done. The whole card opens the mail.
-->
<script lang="ts">
  import { own } from "../api";
  import { when } from "../format";
  import { href } from "../route";
  import { removeWatch, type Watch } from "../watches";

  let { watch, ondone }: { watch: Watch; ondone: (id: string) => void } = $props();

  let busy = $state(false);
  let error = $state("");
  let m = $derived(watch.message);
  let from = $derived(m ? m.from.name || m.from.address || "Unknown sender" : "");

  async function done() {
    busy = true;
    error = "";
    try {
      await removeWatch(watch.id);
      ondone(watch.id);
    } catch (e) {
      // A 404: the watch is gone already (another tab, or the iris command).
      if ((e as { status?: number }).status === 404) return ondone(watch.id);
      error = own(e) || "Iris could not remove the notice. Try again.";
    } finally {
      busy = false;
    }
  }
</script>

<article class="arrived" aria-label="A mail that you watched for came">
  <svg class="mark" viewBox="0 0 24 24" width="24" height="24" aria-hidden="true">
    <circle cx="12" cy="12" r="9" fill="none" stroke="var(--color-primary)" stroke-width="2" />
    <circle class="pupil" cx="12" cy="12" r="3.5" fill="var(--color-primary)" />
  </svg>
  <div class="body">
    <p class="for">You watched for <q>{watch.text}</q></p>
    {#if m}
      <a class="title" href={href.mail(m.id)}>{m.subject || "(No subject)"}</a>
      <p class="detail">{from}{m.date ? `, ${when(m.date)}` : ""}</p>
    {:else}
      <p class="title">The mail came{watch.arrived ? ` on ${when(watch.arrived)}` : ""}, then it left your mailbox.</p>
    {/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
  </div>
  <button type="button" onclick={done} disabled={busy}>Done</button>
</article>

<style>
  .arrived {
    position: relative;
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    padding: var(--space-4);
    border: 1px solid var(--color-border-subtle);
    border-radius: var(--radius-lg);
    background: var(--color-background-elevated);
    transition: border-color var(--transition-fast);
  }
  .arrived:hover {
    border-color: var(--color-primary);
  }
  .mark {
    flex: none;
    margin-top: var(--space-1);
  }
  /* The pupil of the mark grows one time: Iris saw the mail. */
  .pupil {
    transform-origin: 12px 12px;
    animation: seen var(--transition-normal) both;
  }
  @keyframes seen {
    from {
      transform: scale(0);
    }
  }
  .body {
    flex: 1;
    min-width: 0;
  }
  .for {
    margin: 0 0 var(--space-1);
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
    overflow-wrap: anywhere;
  }
  .title {
    display: block;
    margin: 0;
    color: var(--color-text);
    text-decoration: none;
    font-size: var(--font-size-1);
    font-weight: 500;
    overflow-wrap: anywhere;
  }
  /* The title link covers the card, so the whole card opens the mail. */
  a.title::after {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: var(--radius-lg);
  }
  a.title:focus-visible {
    outline: none;
  }
  a.title:focus-visible::after {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
  }
  .detail {
    margin: var(--space-1) 0 0;
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
    overflow-wrap: anywhere;
  }
  .error {
    margin: var(--space-2) 0 0;
    border-left: 4px solid var(--color-error);
    padding-left: var(--space-2);
    font-size: var(--font-size--1);
  }
  button {
    position: relative;
    z-index: 1;
    flex: none;
    padding: var(--space-1) var(--space-3);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-full);
    background: transparent;
    color: var(--color-text);
    font: inherit;
    font-size: var(--font-size--1);
    cursor: pointer;
    transition: border-color var(--transition-fast);
  }
  button:hover:not(:disabled) {
    border-color: var(--color-primary);
  }
  button:disabled {
    opacity: 0.5;
    cursor: default;
  }
  @media (prefers-reduced-motion: reduce) {
    .pupil {
      animation: none;
    }
    .arrived,
    button {
      transition: none;
    }
  }
</style>
