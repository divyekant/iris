<!--
  The setup flow (R1-18): connect one Gmail account, show the copy of the mirror,
  and say that nothing changed in the mailbox. Iris only reads in release 1.
-->
<script lang="ts">
  import { get, type Status } from "../api";

  let { status: initial, poll = 2000 }: { status: Status; poll?: number } = $props();
  // svelte-ignore state_referenced_locally
  let status = $state(initial);

  // Ask the engine for progress until the views close. Each request is local.
  $effect(() => {
    if (!poll) return;
    const timer = setInterval(async () => {
      try {
        status = await get<Status>("status");
      } catch {
        // The engine can restart. Try again at the next tick.
      }
    }, poll);
    return () => clearInterval(timer);
  });

  let account = $derived(status.accounts[0]);
  let m = $derived(status.messages);
  let stage = $derived(!account ? "connect" : account.mirror_ready_at ? "ready" : "copy");
  const number = (n: number) => n.toLocaleString("en-US");
</script>

<h1>Set up Iris</h1>

<ol class="stages">
  <li aria-current={stage === "connect" ? "step" : undefined} class:done={stage !== "connect"}>Connect Gmail</li>
  <li aria-current={stage === "copy" ? "step" : undefined} class:done={stage === "ready"}>Copy the mailbox</li>
  <li aria-current={stage === "ready" ? "step" : undefined}>Read the brief</li>
</ol>

{#if stage === "connect"}
  <section>
    <h2>Connect your Gmail account</h2>
    <p>Iris uses a Google sign-in client of your own. Iris asks only for permission to read.</p>
    <ol>
      <li>
        In the <a href="https://console.cloud.google.com/auth/clients" rel="noreferrer" target="_blank">Google Auth Platform</a>,
        make a project with the Gmail API, the scope <code>gmail.readonly</code> and a client of the type "Desktop app".
      </li>
      <li>Download the JSON file of the client. Google shows the secret only one time.</li>
      <li>On the machine that runs Iris, run <code>iris account add client.json</code>, then sign in.</li>
    </ol>
    <p class="note">This page updates when the account is connected.</p>
  </section>
{:else if stage === "copy"}
  <section>
    <h2>Iris copies {account.address}</h2>
    <progress max={m.total || 1} value={m.fetched} aria-label="Messages copied"></progress>
    <p>{number(m.fetched)} of {m.total ? number(m.total) : "all"} messages copied.</p>
    <p class="note">Iris only reads. Nothing in your mailbox changes. You can close this page: the copy continues.</p>
  </section>
{:else}
  <section>
    <h2>Mirror ready. {number(m.fetched)} messages. Nothing in your mailbox changed.</h2>
    {#if m.understood < m.fetched}
      <progress max={m.fetched || 1} value={m.understood} aria-label="Messages understood"></progress>
      <p>Iris reads the mail and makes the brief: {number(m.understood)} of {number(m.fetched)} messages.</p>
    {/if}
    <p><a class="button" href="#/brief">Open the brief</a></p>
  </section>
{/if}

<style>
  h1 {
    font-size: var(--font-size-3);
    margin: 0 0 var(--space-6);
  }
  h2 {
    font-size: var(--font-size-1);
  }
  .stages {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-6);
    padding: 0;
    margin: 0 0 var(--space-8);
    list-style: none;
    counter-reset: stage;
    color: var(--color-text-muted);
  }
  .stages li {
    counter-increment: stage;
  }
  .stages li::before {
    content: counter(stage) ". ";
  }
  .stages li[aria-current="step"] {
    color: var(--color-text);
    font-weight: 600;
  }
  .stages li.done::after {
    content: " ✓";
    color: var(--color-success);
  }
  section {
    max-width: 60ch;
  }
  li {
    margin-bottom: var(--space-2);
  }
  progress {
    width: 100%;
    height: var(--space-2);
    accent-color: var(--color-primary);
  }
  .note {
    color: var(--color-text-muted);
  }
  .button {
    display: inline-block;
    /* Gold is a border here, never the text or the text background (contrast). */
    border: 2px solid var(--color-primary);
    color: var(--color-text);
    text-decoration: none;
    font-weight: 600;
    padding: var(--space-2) var(--space-4);
    border-radius: var(--radius-md);
  }
</style>
