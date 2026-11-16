<!--
  The setup flow (R1-18): connect one Gmail account, copy the mailbox, read the
  brief. The page says at each step that nothing changes in the mailbox.
-->
<script lang="ts">
  import { get, type Status } from "../api";

  let { status: initial, poll = 2000 }: { status: Status; poll?: number } = $props();
  // svelte-ignore state_referenced_locally
  let status = $state(initial);

  // Ask the local engine for progress while this page is open.
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
  let stage = $derived(!account ? 0 : account.mirror_ready_at ? 2 : 1);
  const number = (n: number) => n.toLocaleString("en-US");
  const STEPS = ["Connect Gmail", "Copy the mailbox", "Read the brief"];
</script>

<div class="setup">
  <p class="hello">Welcome to Iris</p>
  <ol class="steps" aria-label="Steps">
    {#each STEPS as step, i (step)}
      <li class:done={i < stage} aria-current={i === stage ? "step" : undefined}>
        <span class="n" aria-hidden="true">{i < stage ? "✓" : i + 1}</span>{step}
      </li>
    {/each}
  </ol>

  {#if stage === 0}
    <section>
      <h1>Connect your Gmail account</h1>
      <p>Iris signs in with a Google client that you own. It asks Google only for permission to read.</p>
      <ol class="todo">
        <li>
          Open the <a href="https://console.cloud.google.com/auth/clients" rel="noreferrer" target="_blank">Google Auth Platform</a>.
          Make a project with the Gmail API, the scope <code>gmail.readonly</code>, and a client of the type "Desktop app".
        </li>
        <li>Download the JSON file of the client. Google shows its secret only one time.</li>
        <li>On the machine that runs Iris, run <code>iris account add client.json</code> and sign in.</li>
      </ol>
      <p class="quiet">This page goes to the next step when the account is connected.</p>
    </section>
  {:else if stage === 1}
    <section>
      <h1>Iris copies {account.address}</h1>
      <p class="big"><strong>{number(m.fetched)}</strong> of {m.total ? number(m.total) : "all"} messages copied.</p>
      <progress max={m.total || 1} value={m.fetched} aria-label="Messages copied"></progress>
      <p class="quiet">Iris only reads. Nothing in your mailbox changes. You can close this page: the copy continues.</p>
    </section>
  {:else}
    <section>
      <h1>Mirror ready.</h1>
      <p class="big">{number(m.fetched)} messages. Nothing in your mailbox changed.</p>
      {#if m.understood < m.fetched}
        <progress max={m.fetched || 1} value={m.understood} aria-label="Messages read"></progress>
        <p class="quiet">Iris reads the mail to make your brief: {number(m.understood)} of {number(m.fetched)} messages.</p>
      {/if}
      <a class="go" href="#/brief">Open the brief</a>
    </section>
  {/if}
</div>

<style>
  .setup {
    max-width: 560px;
    margin: var(--space-12) auto 0;
  }
  .hello {
    color: var(--color-text-muted);
    margin: 0 0 var(--space-4);
  }
  .steps {
    display: flex;
    flex-wrap: wrap;
    gap: var(--space-2) var(--space-6);
    list-style: none;
    padding: 0;
    margin: 0 0 var(--space-12);
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  .steps li {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  .n {
    display: grid;
    place-items: center;
    width: var(--space-6);
    height: var(--space-6);
    border-radius: var(--radius-full);
    border: 1px solid var(--color-border);
    font-weight: 600;
  }
  .steps li[aria-current="step"] {
    color: var(--color-text);
  }
  .steps li[aria-current="step"] .n {
    border-color: var(--color-primary);
  }
  .steps .done .n {
    color: var(--color-success);
    border-color: var(--color-success);
  }
  h1 {
    font-size: var(--font-size-3);
    font-weight: 500;
    letter-spacing: -0.01em;
    line-height: 1.2;
    margin: 0 0 var(--space-4);
    overflow-wrap: anywhere;
  }
  .big {
    font-size: var(--font-size-1);
    margin: 0 0 var(--space-4);
  }
  .big strong {
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .todo {
    padding-left: var(--space-6);
    margin: var(--space-6) 0;
  }
  .todo li {
    margin-bottom: var(--space-3);
    padding-left: var(--space-1);
  }
  .quiet {
    color: var(--color-text-muted);
  }
  progress {
    width: 100%;
    height: var(--space-2);
    accent-color: var(--color-primary);
    margin-bottom: var(--space-2);
  }
  .go {
    display: inline-block;
    margin-top: var(--space-6);
    background: var(--color-primary);
    color: var(--color-on-primary);
    text-decoration: none;
    font-weight: 600;
    padding: var(--space-3) var(--space-6);
    border-radius: var(--radius-md);
    transition: transform var(--transition-fast);
  }
  .go:hover {
    transform: translateY(-1px);
  }
  .go:focus-visible {
    outline: 2px solid var(--color-text);
    outline-offset: 2px;
  }
  @media (prefers-reduced-motion: reduce) {
    .go {
      transition: none;
    }
    .go:hover {
      transform: none;
    }
  }
</style>
