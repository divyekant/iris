<!--
  A login code that is still good: large digits, one button to copy, and the
  time left. At the moment it expires, the code leaves the screen (spec 1 section 6.6).
-->
<script lang="ts">
  import type { Item } from "../api";
  import { watch } from "../clock";
  import { copyText, liveCode, party, timeLeft } from "../format";
  import { href } from "../route";

  let { item, now: start = new Date() }: { item: Item; now?: Date } = $props();
  // svelte-ignore state_referenced_locally
  let now = $state(start);
  let copied = $state<"" | "done" | "failed">("");

  $effect(() => (item.expires ? watch(item.expires, (d) => (now = d)) : undefined));

  async function copy() {
    copied = (await copyText(item.code ?? "")) ? "done" : "failed";
    setTimeout(() => (copied = ""), 4000);
  }
</script>

{#if liveCode(item, now)}
  <div class="code">
    <p class="digits" aria-label="Login code">{item.code}</p>
    <div class="about">
      <a href={href.item(item.id)}>{party(item.issuer)}</a>
      <span>Ends {timeLeft(item.expires, now)}</span>
    </div>
    <button type="button" onclick={copy}>{copied === "done" ? "Copied" : "Copy"}</button>
    {#if copied === "failed"}<p class="failed" role="status">The browser did not allow the copy. Select the code and copy it.</p>{/if}
  </div>
{/if}

<style>
  .code {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-2) var(--space-6);
    padding: var(--space-4) var(--space-6);
    border: 1px solid var(--color-border);
    border-radius: var(--radius-lg);
    background: var(--color-background-elevated);
  }
  .failed {
    flex-basis: 100%;
    margin: 0;
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  .digits {
    user-select: all;
    margin: 0;
    font-family: var(--font-family-mono);
    font-size: var(--font-size-3);
    letter-spacing: 0.16em;
    font-variant-numeric: tabular-nums;
  }
  .about {
    flex: 1 1 160px;
    min-width: 0;
    display: grid;
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  .about a {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--color-text);
    text-decoration: none;
    font-weight: 500;
  }
  .about a:hover {
    text-decoration: underline;
  }
  @media (max-width: 600px) {
    .code {
      padding: var(--space-4);
    }
    .digits {
      flex-basis: 100%;
    }
  }
  button {
    font: inherit;
    font-weight: 600;
    color: var(--color-on-primary);
    background: var(--color-primary);
    border: 0;
    border-radius: var(--radius-md);
    padding: var(--space-2) var(--space-4);
    min-width: calc(var(--space-24) - var(--space-4));
    cursor: pointer;
  }
  button:focus-visible {
    outline: 2px solid var(--color-text);
    outline-offset: 2px;
  }
</style>
