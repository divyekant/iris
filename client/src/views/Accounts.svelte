<!--
  The accounts (card R6-02): each mailbox that Iris copies, with its state in
  one sentence, and how to add one more. The brief shows the mail of all
  accounts in one list.
-->
<script lang="ts">
  import { get, post, type Account, type Status } from "../api";
  import { poll as every } from "../clock";
  import { accountNames, signIn, when } from "../format";

  let { status: initial, poll = 60000 }: { status: Status; poll?: number } = $props();
  // svelte-ignore state_referenced_locally
  let status = $state(initial);
  $effect(() => (poll ? every(() => get<Status>("status"), (s) => (status = s), poll) : undefined));

  let names = $derived(accountNames(status.accounts));

  // The remove of an account (decision D34): one question in place, as the remove of a rule.
  let asking: number | null = $state(null);
  let busy = $state(false);
  let error = $state("");
  let removed = $state("");

  async function remove(a: Account) {
    busy = true;
    error = "";
    try {
      // The engine answers after the delete of the mail, with the new status.
      status = await post<Status>(`accounts/${a.id}/remove`, { address: a.address });
      removed = status.accounts.some((x) => x.id === a.id) ? "" : a.address;
      asking = null;
    } catch (e) {
      error = (e as Error).message;
    } finally {
      busy = false;
    }
  }

  // The state of one account, in one sentence, and its tone.
  function stateOf(a: Account): { text: string; tone: "ok" | "busy" | "retry" | "stopped" } {
    // An account that the person removed (decision D34): the engine deletes its mail now.
    if (a.state === "removing") return { text: "Iris deletes the mail of this account from this machine.", tone: "busy" };
    if (a.state === "needs_sign_in") return { text: `The copy stopped: ${signIn(a).who} refused the sign-in. Sign in again to continue.`, tone: "stopped" };
    // A failed run comes before the busy states. The status cannot tell if a new try runs now, so the
    // sentence says only what it knows: the last try failed, and Iris tries again. Only a refused sign-in stops.
    // A sync writes "error: ...", a verify writes "N differences, M skipped, then error: ..." (iris-sync).
    const last = a.last_sync;
    if (last?.result?.includes("error:"))
      return { text: `${a.mirror_ready_at ? "The last sync" : "The last try of the first copy"} failed. Iris tries again each minute.`, tone: "retry" };
    // The process of the last sync ended before the sync ended (SYNC_STOPPED of iris-store, issue 710).
    if (last?.result?.startsWith("stopped:"))
      return { text: `${a.mirror_ready_at ? "The last sync" : "The last try of the first copy"} stopped before it ended. Iris tries again each minute.`, tone: "retry" };
    // A running try after a failed try: the failure stays in view (issue 716).
    if (last && !last.ended && !last.result && last.after_failure) return { text: "The last try failed. Iris tries again now.", tone: "retry" };
    // `sync` is set while a first copy or a full copy again runs (iris-sync, account.sync_kind).
    if (!a.mirror_ready_at) return { text: "Iris copies this mailbox now. The brief shows the mail that it copied so far.", tone: "busy" };
    if (a.sync) return { text: "Iris copies this mailbox again now, to find each change.", tone: "busy" };
    // A read-only account copies, but its approved changes wait: not a plain "ok" (issue 681).
    return { text: last?.ended ? `Up to date. Last sync ${when(last.ended)}.` : "Up to date.", tone: a.state === "read_only" ? "retry" : "ok" };
  }
</script>

<h1>Accounts</h1>
<p class="sub">Iris copies each mailbox to this machine. Your brief shows the mail of all accounts in one list.</p>

{#if removed}
  <p class="done" role="status">Removed {removed}. Its mail is gone from Iris on this machine.</p>
{/if}

<ul class="accounts">
  {#each status.accounts as a (a.id)}
    {@const s = stateOf(a)}
    <li>
      <span class="tone {s.tone}" aria-hidden="true"></span>
      <div class="body">
        <p class="address">
          {a.address}{#if names[`a-${a.id}`]}<span class="name">Shown as “{names[`a-${a.id}`]}” in the brief</span>{/if}
        </p>
        <p class="state">{s.text}</p>
        {#if a.state === "needs_sign_in"}
          {@const s = signIn(a)}
          <p class="state">On the machine that runs Iris, run <code>{s.command}</code> {s.detail}.</p>
        {:else if a.state === "read_only"}
          <!-- The copy runs, but Iris has no write access: approved changes wait (issue 681). -->
          <p class="state">Iris can read this mailbox, but it cannot change it. Your approved changes wait.</p>
          <p class="state">To give write access, run <code>iris account add client.json --write</code> on the machine that runs Iris and sign in again.</p>
        {/if}
        {#if asking === a.id}
          <div class="ask" role="group" aria-label={`Remove ${a.address}`}>
            <p>
              Remove {a.address}? Iris deletes its mail from this machine. The mailbox at {a.provider === "gmail" ? "Gmail" : a.provider === "outlook" ? "Outlook" : "your provider"} does not
              change, and you can add it again later. The files of agents in its jobs go. A backup and the notes of jobs keep their copy.
            </p>
            <p class="actions">
              <button type="button" class="danger" onclick={() => remove(a)} disabled={busy}>{busy ? "Removing…" : "Remove"}</button>
              <button type="button" class="quiet" onclick={() => ((asking = null), (error = ""))} disabled={busy}>Keep</button>
            </p>
            {#if error}<p class="error" role="alert">{error}</p>{/if}
          </div>
        {/if}
      </div>
      {#if a.state !== "removing" && asking !== a.id}
        <button type="button" class="quiet remove" onclick={() => ((asking = a.id), (removed = ""))} disabled={busy} aria-label={`Remove ${a.address}`}>Remove</button>
      {/if}
    </li>
  {:else}
    <li class="none">No account yet. <a href="#/setup">Set up Iris</a> to copy your mail.</li>
  {/each}
</ul>

<section class="add" aria-labelledby="add">
  <h2 id="add">Add an account</h2>
  <p>
    On the machine that runs Iris, run <code>iris account add client.json</code> and sign in with the other mailbox. You can use the same client
    file. The new account starts to copy at once, and its mail joins your brief.
  </p>
</section>

<style>
  h1 {
    margin: 0 0 var(--space-2);
    font-size: var(--font-size-3);
  }
  .sub {
    margin: 0 0 var(--space-6);
    color: var(--color-text-muted);
  }
  .accounts {
    list-style: none;
    margin: 0 0 var(--space-8);
    padding: 0;
    border-top: 1px solid var(--color-border);
  }
  .accounts li {
    display: flex;
    gap: var(--space-3);
    align-items: baseline;
    padding: var(--space-4) 0;
    border-bottom: 1px solid var(--color-border);
  }
  .tone {
    flex: none;
    width: var(--space-2);
    height: var(--space-2);
    border-radius: var(--radius-full);
    transform: translateY(-2px);
  }
  .tone.ok {
    background: var(--color-success);
  }
  .tone.busy {
    background: var(--color-primary);
  }
  .tone.retry {
    background: var(--color-warning);
  }
  .tone.stopped {
    background: var(--color-error);
  }
  .body {
    flex: 1;
    min-width: 0;
  }
  button {
    font: inherit;
    border-radius: var(--radius-md);
    padding: var(--space-2) var(--space-4);
    cursor: pointer;
    transition: border-color var(--transition-fast), background var(--transition-fast);
  }
  button:disabled {
    cursor: default;
    opacity: 0.5;
  }
  .quiet,
  .danger {
    color: var(--color-text);
    background: none;
    border: 1px solid var(--color-border);
  }
  .quiet:hover:not(:disabled) {
    border-color: var(--color-text-faint);
  }
  .danger {
    border-color: var(--color-error);
  }
  .danger:hover:not(:disabled) {
    background: var(--color-background-surface);
  }
  /* Quiet until the pointer or the focus is on the line, as the remove of a rule. Always shown on a touch screen. */
  .remove {
    flex: none;
    align-self: center;
    font-size: var(--font-size--1);
    padding: var(--space-1) var(--space-3);
    color: var(--color-text-muted);
    border-color: transparent;
  }
  .accounts li:hover .remove:not(:disabled),
  .remove:focus-visible {
    color: var(--color-text);
    border-color: var(--color-border);
  }
  @media (any-pointer: coarse), (max-width: 600px) {
    button {
      min-height: var(--size-touch);
      min-width: var(--size-touch);
    }
  }
  .ask {
    margin-top: var(--space-3);
    padding: var(--space-3) var(--space-4);
    border-radius: var(--radius-md);
    background: var(--color-background-surface);
  }
  .ask p {
    margin: 0;
  }
  .ask .actions {
    display: flex;
    gap: var(--space-2);
    margin-top: var(--space-3);
  }
  .error {
    margin-top: var(--space-2) !important;
    color: var(--color-error);
  }
  .done {
    margin: 0 0 var(--space-4);
    color: var(--color-text-muted);
  }
  .address {
    margin: 0;
    font-weight: 500;
    overflow-wrap: anywhere;
  }
  .name {
    margin-left: var(--space-2);
    color: var(--color-text-muted);
    font-weight: 400;
    font-size: var(--font-size--1);
  }
  .state {
    margin: var(--space-1) 0 0;
    color: var(--color-text-muted);
    font-size: var(--font-size--1);
  }
  .add h2 {
    margin: 0 0 var(--space-2);
    font-size: var(--font-size-1);
  }
  .add p {
    margin: 0;
    color: var(--color-text-muted);
  }
</style>
