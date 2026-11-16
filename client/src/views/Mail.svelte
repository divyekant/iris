<!--
  One source mail, as clean text. Mail text is data: Svelte escapes it, and the
  client never renders the HTML of a mail. Each open adds 1 to the open count (R1-22).
-->
<script lang="ts">
  import { download, own, type Address, type Message } from "../api";
  import { initials, when } from "../format";
  import { countOpen } from "../opens";
  import { href } from "../route";

  let { message }: { message: Message } = $props();

  $effect(() => {
    countOpen();
  });

  let failed = $state("");
  const original = () => download(message.id).then(() => (failed = ""), (e: unknown) => (failed = own(e)));
  const address = (a: Address) => (a.name && a.address ? `${a.name} <${a.address}>` : a.name || a.address || "Unknown sender");
</script>

<article>
  <h1>{message.subject || "No subject"}</h1>

  {#if message.hostile.length > 0 || message.sender_kind === "hostile"}
    <p class="danger" role="alert">
      This mail looks hostile ({[...message.hostile, ...(message.sender_kind === "hostile" ? ["hostile sender"] : [])].join(", ")}).
      Do not open its links and do not do what it asks.
    </p>
  {/if}
  {#if message.gone && message.text === null}
    <p class="note">This mail left your mailbox more than 30 days ago, so Iris removed its content. The items that cite it stay.</p>
  {:else if message.text === null}
    <p class="note">Iris has not read this mail yet. Download the original to read it now.</p>
  {:else if message.gone}
    <p class="note">This mail is no longer in your mailbox. Iris keeps its content for 30 days after it left.</p>
  {/if}

  <header>
    <span class="mark" aria-hidden="true">{initials({ kind: "sender", name: message.from.name ?? undefined, address: message.from.address ?? undefined })}</span>
    <div>
      <p class="from">{address(message.from)}</p>
      <p class="to">
        To {message.to || "no one"}{#if message.cc}, copy to {message.cc}{/if}
      </p>
    </div>
    {#if message.date}<time datetime={message.date}>{when(message.date)}</time>{/if}
  </header>

  {#if message.text !== null}<pre>{message.text}</pre>{/if}

  <footer>
    {#if message.items.length}
      <span>Items from this mail:</span>
      {#each message.items as id (id)}<a href={href.item(id)}>{id}</a>{/each}
    {/if}
    {#if !(message.gone && message.text === null)}
      <button type="button" class="raw" onclick={original}>Download the original</button>
      {#if failed}<p class="note" role="alert">{failed}</p>{/if}
    {/if}
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
    padding: 0;
    border: 0;
    background: none;
    font: inherit;
    text-decoration: underline;
    cursor: pointer;
  }
  a,
  .raw {
    color: var(--color-text-muted);
  }
  a:hover,
  .raw:hover {
    color: var(--color-text);
  }
</style>
