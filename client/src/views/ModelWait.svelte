<!--
  "Try now" (decision D35, issue 136): after model errors, Iris waits before
  it calls the model again. This note says until when, and the button ends
  the wait at once. The backoff stays: a new error starts a longer wait.
-->
<script lang="ts">
  import { postStatus, readStatus, type Status } from "../api";
  import { when } from "../format";

  // `onstatus` gets each status that this note reads, or null when a newer status answered first
  // (api.ts): then the brief keeps the newer one.
  let { at, onstatus }: { at: string; onstatus: (s: Status | null) => void } = $props();
  let busy = $state(false);
  let error = $state("");
  // At `at` the wait is over: the note goes, and a fresh status tells if a new wait started.
  let over = $state(false);
  $effect(() => {
    over = false;
    const timer = setTimeout(
      () => {
        over = true;
        // A failed read leaves the next poll of the page to read the status.
        readStatus().then(onstatus, () => {});
      },
      Math.max(0, Date.parse(at) - Date.now()),
    );
    return () => clearTimeout(timer);
  });

  async function tryNow() {
    busy = true;
    error = "";
    try {
      onstatus(await postStatus("model/try", {}));
      // A read that started during the try can show the wait from before it: read again.
      onstatus(await readStatus().catch(() => null));
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }
</script>

{#if !over}<div class="model-wait" role="status">
  <p>The model failed, so Iris waits before it reads more mail. It tries again at {when(at)}.</p>
  <button type="button" onclick={tryNow} disabled={busy}>{busy ? "Trying…" : "Try now"}</button>
  {#if error}<p class="error" role="alert">{error}</p>{/if}
</div>{/if}

<style>
  .model-wait {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2) var(--space-4);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    padding: var(--space-3) var(--space-4);
    margin: var(--space-6) 0;
  }
  p {
    margin: 0;
    flex: 1 1 16rem;
  }
  button {
    font: inherit;
    min-height: 44px;
    color: var(--color-text);
    background: none;
    border: 1px solid var(--color-border);
    border-radius: var(--radius-md);
    padding: var(--space-2) var(--space-4);
    cursor: pointer;
    transition: border-color var(--transition-fast);
  }
  button:hover:not(:disabled) {
    border-color: var(--color-text-faint);
  }
  button:disabled {
    cursor: default;
    opacity: 0.5;
  }
  .error {
    flex-basis: 100%;
    color: var(--color-error);
  }
</style>
