<!--
  Notices (card R5-08): each notice that is open, above every view. A notice comes
  from a `notify` rule of the person (the card opens the mail), or from a job that
  needs the person (the card opens the job, docs/specs/2026-11-25-job-notices.md).
  On a phone, the notices fold to one line with a count, so each view starts at the
  top. A tap on the line opens them (decision D38, issue 452).
-->
<script lang="ts">
  import { wrote } from "../api";
  import { follow } from "../clock";
  import { when } from "../format";
  import { doneNotice, loadNotices, NOTICE_POLL, type Notice } from "../notices";
  import { href } from "../route";

  let { first, poll = NOTICE_POLL }: { first?: Notice[]; poll?: number } = $props();

  // svelte-ignore state_referenced_locally
  let notices = $state<Notice[]>(first ?? []);
  let gone = $state(new Set<string>());
  let busy = $state("");
  let error = $state("");
  // The first three show. The rest wait behind one line.
  const SHOW = 3;
  let open = $state(false);
  let list = $derived(notices.filter((n) => !gone.has(n.id)));
  let shown = $derived(open ? list : list.slice(0, SHOW));

  $effect(() => {
    if (!poll) return;
    return follow(loadNotices, (n) => (notices = n), poll);
  });

  async function done(n: Notice) {
    busy = n.id;
    error = "";
    try {
      await doneNotice(n.id);
      gone = new Set([...gone, n.id]);
    } catch (e) {
      // A 404: the notice is gone already.
      if ((e as { status?: number }).status === 404) gone = new Set([...gone, n.id]);
      else error = wrote(e, "closed the notice", "Iris could not close the notice. Try again.");
    } finally {
      busy = "";
    }
  }
</script>

<!-- Each new view starts with the notices folded again: a phone shows the line, a wide screen three cards. -->
<svelte:window onhashchange={() => (open = false)} />

{#if list.length}
  <section class="notices" class:folded={!open} aria-label="Notices">
    <button class="line" type="button" aria-expanded="false" onclick={() => (open = true)}>
      <span class="dot" aria-hidden="true"></span>
      <span class="count">{list.length} {list.length === 1 ? "notice" : "notices"}</span>
      <span class="show">Show</span>
    </button>
    {#each shown as n (n.id)}
      <article class="notice">
        <span class="dot" aria-hidden="true"></span>
        <div class="body">
          <p class="rule">{n.rule_name || `Rule ${n.rule}`}</p>
          {#if n.job}
            <a class="title" href={href.job(n.job)}>{n.title}: {n.text}</a>
          {:else if n.message}
            <a class="title" href={href.mail(n.message.id)}>{n.title}: {n.text}</a>
          {:else}
            <p class="title">{n.title}: {n.text}</p>
          {/if}
          <p class="detail">{when(n.created)}{n.message || n.job ? "" : ". The mail left your mailbox."}</p>
        </div>
        <button type="button" onclick={() => done(n)} disabled={busy === n.id}>Done</button>
      </article>
    {/each}
    {#if list.length > SHOW && !open}
      <button class="more" type="button" onclick={() => (open = true)}>Show {list.length - SHOW} more</button>
    {/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
  </section>
{/if}

<style>
  .notices {
    display: grid;
    gap: var(--space-2);
    margin-bottom: var(--space-8);
  }
  .notice {
    position: relative;
    display: flex;
    align-items: flex-start;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--color-border-subtle);
    border-left: 4px solid var(--color-primary);
    border-radius: var(--radius-md);
    background: var(--color-background-elevated);
    transition: border-color var(--transition-fast);
  }
  .notice:hover {
    border-color: var(--color-primary);
  }
  .dot {
    flex: none;
    width: 8px;
    height: 8px;
    margin-top: var(--space-2);
    border-radius: var(--radius-full);
    background: var(--color-primary);
  }
  .body {
    flex: 1;
    min-width: 0;
  }
  .rule,
  .detail {
    margin: 0;
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
    overflow-wrap: anywhere;
  }
  .title {
    display: block;
    margin: var(--space-1) 0;
    color: var(--color-text);
    text-decoration: none;
    font-weight: 500;
    overflow-wrap: anywhere;
  }
  /* The title link covers the card, so the whole card opens the mail. */
  a.title::after {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: var(--radius-md);
  }
  a.title:focus-visible {
    outline: none;
  }
  a.title:focus-visible::after {
    outline: 2px solid var(--color-primary);
    outline-offset: 2px;
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
  .more {
    justify-self: start;
  }
  .line {
    display: none;
  }
  /* A phone: one line until the person opens the notices. */
  @media (max-width: 600px) {
    .folded .notice,
    .folded .more {
      display: none;
    }
    .folded .line {
      display: flex;
      align-items: center;
      gap: var(--space-3);
      min-height: var(--size-touch);
      padding: var(--space-2) var(--space-4);
      border: 1px solid var(--color-border-subtle);
      border-left: 4px solid var(--color-primary);
      border-radius: var(--radius-md);
      background: var(--color-background-elevated);
      font-size: inherit;
      text-align: left;
    }
    .line .dot {
      margin-top: 0;
    }
    .count {
      flex: 1;
      min-width: 0;
      font-weight: 500;
      overflow-wrap: anywhere;
    }
    .show {
      color: var(--color-text-muted);
      font-size: var(--font-size--1);
    }
  }
  .error {
    margin: 0;
    border-left: 4px solid var(--color-error);
    padding-left: var(--space-2);
    font-size: var(--font-size--1);
  }
  @media (prefers-reduced-motion: reduce) {
    .notice,
    button {
      transition: none;
    }
  }
</style>
