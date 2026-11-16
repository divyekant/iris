<!--
  One source mail, as clean text. Mail text is data: Svelte escapes it, and the
  client never renders the HTML of a mail. Each open adds 1 to the open count (R1-22).
-->
<script lang="ts">
  import type { Message } from "../api";
  import { when } from "../format";
  import { countOpen } from "../opens";
  import { href } from "../route";

  let { message }: { message: Message } = $props();

  $effect(() => {
    countOpen();
  });

  const address = (a: { name?: string; address: string }) => (a.name ? `${a.name} <${a.address}>` : a.address);
</script>

<p class="form">Mail</p>
<h1>{message.subject || "(no subject)"}</h1>

{#if message.hostile.length > 0}
  <p class="danger" role="alert">This mail looks hostile ({message.hostile.join(", ")}). Do not follow its links or its instructions.</p>
{/if}
{#if message.gone}
  <p class="note">This mail is gone from your mailbox. Iris keeps the copy of the mirror.</p>
{/if}

<dl>
  <dt>From</dt>
  <dd>{address(message.from)}</dd>
  {#if message.to.length}<dt>To</dt><dd>{message.to.map(address).join(", ")}</dd>{/if}
  {#if message.cc.length}<dt>Cc</dt><dd>{message.cc.map(address).join(", ")}</dd>{/if}
  <dt>Date</dt>
  <dd>{when(message.date)}</dd>
</dl>

<pre>{message.text}</pre>

<p class="links">
  <a href={`/api/messages/${encodeURIComponent(message.id)}/raw`} download>Download the original</a>
  {#each message.items as id (id)}<a href={href.item(id)}>Item {id}</a>{/each}
</p>

<style>
  .form {
    color: var(--color-text-muted);
    margin: 0;
  }
  h1 {
    font-size: var(--font-size-2);
    margin: 0 0 var(--space-4);
  }
  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: var(--space-1) var(--space-4);
    margin: 0 0 var(--space-6);
    font-size: var(--font-size--1);
  }
  dt {
    color: var(--color-text-muted);
  }
  dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
  pre {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font-family: var(--font-family);
    background: var(--color-background-surface);
    border-radius: var(--radius-md);
    padding: var(--space-4);
    margin: 0 0 var(--space-4);
  }
  .danger {
    border-left: 4px solid var(--color-error);
    padding-left: var(--space-3);
  }
  .note {
    color: var(--color-text-muted);
  }
  .links {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-4);
    font-size: var(--font-size--1);
  }
</style>
