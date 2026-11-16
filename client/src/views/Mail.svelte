<!--
  One source mail, as clean text. Mail text is data: Svelte escapes it, and the
  client never renders the HTML of a mail. Each open adds 1 to the open count (R1-22).
-->
<script lang="ts">
  import type { Message } from "../api";
  import { initials, when } from "../format";
  import { countOpen } from "../opens";
  import { href } from "../route";

  let { message }: { message: Message } = $props();

  $effect(() => {
    countOpen();
  });

  const address = (a: { name?: string; address: string }) => (a.name ? `${a.name} <${a.address}>` : a.address);
</script>

<article>
  <h1>{message.subject || "No subject"}</h1>

  {#if message.hostile.length > 0}
    <p class="danger" role="alert">
      This mail looks hostile ({message.hostile.join(", ")}). Do not open its links and do not do what it asks.
    </p>
  {/if}
  {#if message.gone}
    <p class="note">This mail is no longer in your mailbox. Iris keeps its copy in the mirror.</p>
  {/if}

  <header>
    <span class="mark" aria-hidden="true">{initials({ kind: "sender", ...message.from })}</span>
    <div>
      <p class="from">{address(message.from)}</p>
      <p class="to">
        To {message.to.map(address).join(", ") || "no one"}{#if message.cc.length}, copy to {message.cc.map(address).join(", ")}{/if}
      </p>
    </div>
    <time datetime={message.date}>{when(message.date)}</time>
  </header>

  <pre>{message.text}</pre>

  <footer>
    {#if message.items.length}
      <span>Items from this mail:</span>
      {#each message.items as id (id)}<a href={href.item(id)}>{id}</a>{/each}
    {/if}
    <a class="raw" href={`/api/messages/${encodeURIComponent(message.id)}/raw`} download>Download the original</a>
  </footer>
</article>

<style>
  article {
    max-width: 680px;
  }
  h1 {
    font-size: var(--font-size-2);
    font-weight: 500;
    line-height: 1.3;
    margin: var(--space-4) 0 var(--space-6);
  }
  header {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    margin-bottom: var(--space-6);
    font-size: var(--font-size--1);
  }
  header div {
    flex: 1;
    min-width: 0;
  }
  header p {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .from {
    font-weight: 500;
  }
  .to,
  time {
    color: var(--color-text-muted);
  }
  .mark {
    flex: none;
    display: grid;
    place-items: center;
    width: var(--space-8);
    height: var(--space-8);
    border-radius: var(--radius-full);
    background: var(--color-background-surface);
    color: var(--color-text-muted);
    font-weight: 600;
  }
  pre {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font-family: var(--font-family);
    font-size: var(--font-size-0);
    line-height: 1.6;
    margin: 0 0 var(--space-8);
  }
  .danger {
    border-left: 4px solid var(--color-error);
    padding-left: var(--space-3);
  }
  .note {
    color: var(--color-text-muted);
  }
  footer {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--space-2) var(--space-3);
    padding-top: var(--space-4);
    border-top: 1px solid var(--color-border-subtle);
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  .raw {
    margin-left: auto;
  }
  a {
    color: var(--color-text-muted);
  }
  a:hover {
    color: var(--color-text);
  }
</style>
